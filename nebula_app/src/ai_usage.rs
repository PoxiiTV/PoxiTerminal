//! Uso de tokens de una sesión de CLI de IA (claude / codex): lee el
//! transcript JSONL de forma incremental y saca tokens, % de contexto,
//! coste estimado e imágenes.
//!
//! # Formatos (verificados contra transcripts reales)
//!
//! - Claude: cada bloque de contenido del asistente es una línea aparte con el
//!   mismo `message.id` y el uso repetido o creciente; se cuenta cada id una
//!   sola vez (acumulando deltas). Las imágenes pegadas se numeran con
//!   `imagePasteIds` (el texto rara vez lleva `[Image #N]`).
//! - Codex: `token_count` trae totales acumulados (se reemplazan, no se suman)
//!   y la ventana de contexto; `turn_context` trae el modelo.

use std::collections::HashMap;
use std::fs::File;
use std::io::{self, BufRead, BufReader, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use base64::Engine as _;
use serde_json::Value;

/// Imágenes que se guardan como mucho (se descartan las más antiguas).
const MAX_IMAGES: usize = 40;
/// Imágenes decodificadas más grandes que esto se ignoran.
const MAX_IMAGE_BYTES: usize = 20 * 1024 * 1024;
const ONE_MILLION: u64 = 1_000_000;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum TranscriptFormat {
    Claude,
    Codex,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct UsageSnapshot {
    pub model: Option<String>,
    /// Tokens ocupados en la ventana de contexto según el último mensaje.
    pub context_tokens: u64,
    pub context_window: Option<u64>,
    /// Acumulado, sin caché.
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cache_read_tokens: u64,
    pub cache_write_tokens: u64,
    /// Coste estimado en USD a precio de API; None si el modelo no está en la tabla.
    pub cost_usd: Option<f64>,
}

impl UsageSnapshot {
    /// Porcentaje de contexto usado (0..=100); None sin ventana conocida.
    pub fn context_percent(&self) -> Option<u8> {
        let window = self.context_window.filter(|&w| w > 0)?;
        Some((self.context_tokens.saturating_mul(100) / window).min(100) as u8)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ImageOrigin {
    Sent,
    Read,
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct SessionImage {
    pub origin: ImageOrigin,
    /// "Image #3" para las enviadas por el usuario; ruta del archivo para las leídas (Read), si se conoce.
    pub label: Option<String>,
    pub media_type: String,
    pub bytes: Arc<Vec<u8>>,
}

/// Precio en USD por millón de tokens.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Price {
    pub input: f64,
    pub output: f64,
    pub cache_write: f64,
    pub cache_read: f64,
}

impl Price {
    fn cost(&self, input: u64, output: u64, cache_write: u64, cache_read: u64) -> f64 {
        (input as f64 * self.input
            + output as f64 * self.output
            + cache_write as f64 * self.cache_write
            + cache_read as f64 * self.cache_read)
            / ONE_MILLION as f64
    }
}

// ponytail: precios copiados a mano de la web de Anthropic; hay que actualizar
// la tabla cuando cambien o salgan modelos nuevos.
// (prefijo del id, input, output, cache_read, ventana de contexto)
const MODELS: &[(&str, f64, f64, f64, u64)] = &[
    ("claude-fable-5-1", 10.0, 50.0, 0.25, ONE_MILLION),
    ("claude-fable-5", 10.0, 50.0, 0.25, ONE_MILLION),
    ("claude-mythos-5", 10.0, 50.0, 0.25, ONE_MILLION),
    ("claude-opus-5-5", 4.0, 20.0, 0.20, ONE_MILLION),
    ("claude-opus-5", 5.0, 25.0, 0.50, ONE_MILLION),
    ("claude-opus-4-8", 5.0, 25.0, 0.50, ONE_MILLION),
    ("claude-opus-4-7", 5.0, 25.0, 0.50, ONE_MILLION),
    ("claude-opus-4-6", 5.0, 25.0, 0.50, ONE_MILLION),
    ("claude-opus-4-5", 5.0, 25.0, 0.50, ONE_MILLION),
    ("claude-sonnet-5-5", 2.0, 10.0, 0.20, ONE_MILLION),
    ("claude-sonnet-5", 2.0, 10.0, 0.20, ONE_MILLION),
    ("claude-sonnet-4-6", 3.0, 15.0, 0.30, ONE_MILLION),
    ("claude-sonnet-4-5", 3.0, 15.0, 0.30, 200_000),
    ("claude-sonnet-4", 3.0, 15.0, 0.30, 200_000),
    ("claude-haiku-4-5", 1.0, 5.0, 0.10, 200_000),
];

/// Entrada de la tabla con el prefijo más largo que casa (el sufijo de fecha
/// y el "[1m]" quedan fuera del prefijo, así que no hace falta quitarlos).
fn model_entry(model: &str) -> Option<&'static (&'static str, f64, f64, f64, u64)> {
    MODELS.iter().filter(|entry| model.starts_with(entry.0)).max_by_key(|entry| entry.0.len())
}

pub(crate) fn price_per_million(model: &str) -> Option<Price> {
    model_entry(model).map(|&(_, input, output, cache_read, _)| Price {
        input,
        output,
        cache_write: input * 1.25,
        cache_read,
    })
}

pub(crate) fn context_window_for(model: &str) -> Option<u64> {
    let window = model_entry(model)?.4;
    Some(if model.contains("[1m]") { ONE_MILLION } else { window })
}

pub(crate) struct UsageTracker {
    path: PathBuf,
    format: TranscriptFormat,
    /// Bytes ya procesados (siempre al final de una línea completa).
    offset: u64,
    snapshot: UsageSnapshot,
    images: Vec<SessionImage>,
    /// Claude: uso máximo visto por message.id → [input, cache_write, cache_read, output].
    seen_messages: HashMap<String, [u64; 4]>,
    /// Claude: tool_use id de un Read → file_path, para etiquetar su imagen.
    read_paths: HashMap<String, String>,
    cost: f64,
    cost_unknown: bool,
}

impl UsageTracker {
    pub fn new(path: PathBuf) -> Self {
        let format = if path.to_string_lossy().contains(".codex") {
            TranscriptFormat::Codex
        } else {
            TranscriptFormat::Claude
        };
        Self {
            path,
            format,
            offset: 0,
            snapshot: UsageSnapshot::default(),
            images: Vec::new(),
            seen_messages: HashMap::new(),
            read_paths: HashMap::new(),
            cost: 0.0,
            cost_unknown: false,
        }
    }

    /// Lee solo lo añadido desde la última llamada. Tolera una última línea a medias
    /// (la deja para la siguiente llamada) y reinicia si el archivo encoge.
    pub fn poll(&mut self) -> io::Result<()> {
        let mut file = File::open(&self.path)?;
        let len = file.metadata()?.len();
        if len < self.offset {
            // Archivo truncado/reescrito: empezar de cero.
            let path = std::mem::take(&mut self.path);
            *self = Self::new(path);
        }
        if len == self.offset {
            return Ok(());
        }
        file.seek(SeekFrom::Start(self.offset))?;
        let mut reader = BufReader::new(file);
        let mut line = Vec::new();
        loop {
            line.clear();
            let read = reader.read_until(b'\n', &mut line)?;
            // Línea a medias: no avanzamos el offset, se relee completa luego.
            if read == 0 || line.last() != Some(&b'\n') {
                break;
            }
            self.offset += read as u64;
            self.ingest(&line);
        }
        Ok(())
    }

    pub fn snapshot(&self) -> &UsageSnapshot {
        &self.snapshot
    }

    pub fn images(&self) -> &[SessionImage] {
        &self.images
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    fn ingest(&mut self, line: &[u8]) {
        // Filtro barato antes de parsear: la mayoría de líneas no interesan.
        let wanted: &[&[u8]] = match self.format {
            TranscriptFormat::Claude => &[b"\"assistant\"", b"\"user\""],
            TranscriptFormat::Codex => &[b"turn_context", b"token_count", b"input_image"],
        };
        if !wanted.iter().any(|needle| contains(line, needle)) {
            return;
        }
        let Ok(value) = serde_json::from_slice::<Value>(line) else {
            return;
        };
        match self.format {
            TranscriptFormat::Claude => self.ingest_claude(&value),
            TranscriptFormat::Codex => self.ingest_codex(&value),
        }
    }

    fn ingest_claude(&mut self, value: &Value) {
        let message = &value["message"];
        match value["type"].as_str() {
            Some("assistant") => self.claude_assistant(value, message),
            Some("user") => self.claude_user(value, message),
            _ => {},
        }
    }

    fn claude_assistant(&mut self, value: &Value, message: &Value) {
        let Some(model) = message["model"].as_str() else {
            return;
        };
        if model == "<synthetic>" {
            return;
        }
        for item in message["content"].as_array().into_iter().flatten() {
            if item["type"] == "tool_use" && item["name"] == "Read" {
                if let (Some(id), Some(file)) =
                    (item["id"].as_str(), item["input"]["file_path"].as_str())
                {
                    self.read_paths.insert(id.to_owned(), file.to_owned());
                }
            }
        }

        let usage = &message["usage"];
        if !usage.is_object() {
            return;
        }
        let field = |key: &str| usage[key].as_u64().unwrap_or(0);
        let now = [
            field("input_tokens"),
            field("cache_creation_input_tokens"),
            field("cache_read_input_tokens"),
            field("output_tokens"),
        ];
        // Mismo id en varias líneas: solo cuenta lo que haya crecido.
        let (delta, latest) = match message["id"].as_str() {
            Some(id) => {
                let seen = self.seen_messages.entry(id.to_owned()).or_default();
                let mut delta = [0; 4];
                for i in 0..4 {
                    let max = seen[i].max(now[i]);
                    delta[i] = max - seen[i];
                    seen[i] = max;
                }
                (delta, *seen)
            },
            None => (now, now),
        };

        let snapshot = &mut self.snapshot;
        snapshot.input_tokens += delta[0];
        snapshot.cache_write_tokens += delta[1];
        snapshot.cache_read_tokens += delta[2];
        snapshot.output_tokens += delta[3];
        match price_per_million(model) {
            Some(price) => self.cost += price.cost(delta[0], delta[3], delta[1], delta[2]),
            None => self.cost_unknown = true,
        }
        snapshot.cost_usd = (!self.cost_unknown).then_some(self.cost);

        // Los subagentes en línea no reflejan el contexto de la sesión principal.
        if value["isSidechain"] != true {
            snapshot.model = Some(model.to_owned());
            snapshot.context_tokens = latest.iter().sum();
            snapshot.context_window = context_window_for(model)
                .map(|window| if snapshot.context_tokens > window { ONE_MILLION } else { window });
        }
    }

    fn claude_user(&mut self, value: &Value, message: &Value) {
        let Some(items) = message["content"].as_array() else {
            return;
        };
        // Numeración: imagePasteIds si cuadra con las imágenes; si no, los
        // marcadores "[Image #N]" del texto, en orden.
        let sent = items.iter().filter(|item| item["type"] == "image").count();
        let paste_ids: Vec<u64> = value["imagePasteIds"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(Value::as_u64)
            .collect();
        let labels = if sent > 0 && paste_ids.len() == sent {
            paste_ids.iter().map(|n| format!("Image #{n}")).collect()
        } else {
            text_markers(items, "text", "text")
        };
        let mut labels = labels.into_iter();

        for item in items {
            match item["type"].as_str() {
                Some("image") => {
                    self.push_claude_image(ImageOrigin::Sent, labels.next(), &item["source"])
                },
                Some("tool_result") => {
                    let file =
                        item["tool_use_id"].as_str().and_then(|id| self.read_paths.remove(id));
                    for part in item["content"].as_array().into_iter().flatten() {
                        if part["type"] == "image" {
                            self.push_claude_image(
                                ImageOrigin::Read,
                                file.clone(),
                                &part["source"],
                            );
                        }
                    }
                },
                _ => {},
            }
        }
    }

    fn push_claude_image(&mut self, origin: ImageOrigin, label: Option<String>, source: &Value) {
        if source["type"] != "base64" {
            return;
        }
        if let (Some(media_type), Some(data)) =
            (source["media_type"].as_str(), source["data"].as_str())
        {
            self.push_image(origin, label, media_type, data);
        }
    }

    fn ingest_codex(&mut self, value: &Value) {
        let payload = &value["payload"];
        match (value["type"].as_str(), payload["type"].as_str()) {
            (Some("turn_context"), _) => {
                if let Some(model) = payload["model"].as_str() {
                    self.snapshot.model = Some(model.to_owned());
                    self.refresh_codex_cost();
                }
            },
            (Some("event_msg"), Some("token_count")) => {
                let info = &payload["info"];
                if !info.is_object() {
                    return;
                }
                let total = &info["total_token_usage"];
                let field = |key: &str| total[key].as_u64().unwrap_or(0);
                // Totales acumulados: se reemplazan. cached va dentro de input.
                let cached = field("cached_input_tokens");
                let snapshot = &mut self.snapshot;
                snapshot.input_tokens = field("input_tokens").saturating_sub(cached);
                snapshot.cache_read_tokens = cached;
                snapshot.output_tokens = field("output_tokens");
                if let Some(context) = info["last_token_usage"]["total_tokens"].as_u64() {
                    snapshot.context_tokens = context;
                }
                if let Some(window) = info["model_context_window"].as_u64() {
                    snapshot.context_window = Some(window);
                }
                self.refresh_codex_cost();
            },
            (Some("response_item"), Some("message")) if payload["role"] == "user" => {
                let Some(items) = payload["content"].as_array() else {
                    return;
                };
                let mut labels = text_markers(items, "input_text", "text").into_iter();
                for item in items {
                    if item["type"] != "input_image" {
                        continue;
                    }
                    let label = labels.next();
                    // "data:image/png;base64,...."
                    let Some((media_type, data)) = item["image_url"]
                        .as_str()
                        .and_then(|url| url.strip_prefix("data:"))
                        .and_then(|url| url.split_once(";base64,"))
                    else {
                        continue;
                    };
                    self.push_image(ImageOrigin::Sent, label, media_type, data);
                }
            },
            _ => {},
        }
    }

    fn refresh_codex_cost(&mut self) {
        let snapshot = &mut self.snapshot;
        snapshot.cost_usd = snapshot.model.as_deref().and_then(price_per_million).map(|price| {
            price.cost(
                snapshot.input_tokens,
                snapshot.output_tokens,
                snapshot.cache_write_tokens,
                snapshot.cache_read_tokens,
            )
        });
    }

    fn push_image(
        &mut self,
        origin: ImageOrigin,
        label: Option<String>,
        media_type: &str,
        data: &str,
    ) {
        if data.len() / 4 * 3 > MAX_IMAGE_BYTES {
            return;
        }
        let Ok(bytes) = base64::engine::general_purpose::STANDARD.decode(data) else {
            return;
        };
        if self.images.len() >= MAX_IMAGES {
            self.images.remove(0);
        }
        self.images.push(SessionImage {
            origin,
            label,
            media_type: media_type.to_owned(),
            bytes: Arc::new(bytes),
        });
    }
}

fn contains(haystack: &[u8], needle: &[u8]) -> bool {
    haystack.windows(needle.len()).any(|window| window == needle)
}

/// Marcadores "[Image #N]" (como "Image #N") de los bloques de texto, en orden.
fn text_markers(items: &[Value], text_type: &str, text_key: &str) -> Vec<String> {
    let mut labels = Vec::new();
    for item in items.iter().filter(|item| item["type"] == text_type) {
        let mut rest = item[text_key].as_str().unwrap_or_default();
        while let Some(start) = rest.find("[Image #") {
            rest = &rest[start + "[Image #".len()..];
            let digits = rest.bytes().take_while(u8::is_ascii_digit).count();
            if digits > 0 && rest[digits..].starts_with(']') {
                labels.push(format!("Image #{}", &rest[..digits]));
            }
        }
    }
    labels
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn b64(bytes: &[u8]) -> String {
        base64::engine::general_purpose::STANDARD.encode(bytes)
    }

    fn write(path: &Path, text: &str) {
        std::fs::write(path, text).unwrap();
    }

    fn append(path: &Path, text: &str) {
        let mut file = std::fs::OpenOptions::new().append(true).open(path).unwrap();
        file.write_all(text.as_bytes()).unwrap();
    }

    fn assistant(id: &str, model: &str, usage: [u64; 4]) -> String {
        serde_json::json!({
            "type": "assistant",
            "requestId": "req",
            "message": {
                "id": id,
                "model": model,
                "usage": {
                    "input_tokens": usage[0],
                    "cache_creation_input_tokens": usage[1],
                    "cache_read_input_tokens": usage[2],
                    "output_tokens": usage[3],
                },
                "content": [{"type": "text", "text": "hola"}],
            },
        })
        .to_string()
            + "\n"
    }

    fn tracker_for(dir: &Path, name: &str, text: &str) -> UsageTracker {
        let path = dir.join(name);
        write(&path, text);
        let mut tracker = UsageTracker::new(path);
        tracker.poll().unwrap();
        tracker
    }

    #[test]
    fn claude_duplicate_ids_counted_once_with_opus_5_5_cost() {
        let dir = tempfile::tempdir().unwrap();
        let text = assistant("m1", "claude-opus-5-5", [10, 100, 1000, 5])
            + &assistant("m1", "claude-opus-5-5", [10, 100, 1000, 50])
            + &assistant("x", "<synthetic>", [999, 999, 999, 999])
            + "no es json\n"
            + &assistant("m2", "claude-opus-5-5", [20, 0, 2000, 30]);
        let tracker = tracker_for(dir.path(), "s.jsonl", &text);
        let s = tracker.snapshot();
        assert_eq!(tracker.format, TranscriptFormat::Claude);
        assert_eq!(
            (s.input_tokens, s.cache_write_tokens, s.cache_read_tokens, s.output_tokens),
            (30, 100, 3000, 80)
        );
        assert_eq!(s.model.as_deref(), Some("claude-opus-5-5"));
        assert_eq!(s.context_tokens, 2050);
        assert_eq!(s.context_window, Some(ONE_MILLION));
        assert_eq!(s.context_percent(), Some(0));
        // 30*4 + 80*20 + 100*5 + 3000*0.2 = 2820 por millón
        assert!((s.cost_usd.unwrap() - 0.00282).abs() < 1e-12);
    }

    #[test]
    fn unknown_model_has_no_cost_or_window() {
        let dir = tempfile::tempdir().unwrap();
        let tracker =
            tracker_for(dir.path(), "s.jsonl", &assistant("m1", "claude-foo-9", [1, 2, 3, 4]));
        let s = tracker.snapshot();
        assert_eq!(s.cost_usd, None);
        assert_eq!(s.context_window, None);
        assert_eq!(s.context_percent(), None);
        assert_eq!(s.output_tokens, 4);
    }

    #[test]
    fn model_table_lookup() {
        assert_eq!(context_window_for("claude-sonnet-4-20250514"), Some(200_000));
        assert_eq!(context_window_for("claude-sonnet-4-5-20250929[1m]"), Some(ONE_MILLION));
        assert_eq!(context_window_for("claude-haiku-4-5-20251001"), Some(200_000));
        assert_eq!(price_per_million("claude-opus-5-5").unwrap().cache_write, 5.0);
        assert_eq!(price_per_million("claude-opus-5").unwrap().input, 5.0);
        assert_eq!(price_per_million("gpt-5.4"), None);
        // Contexto por encima de la ventana de la tabla → 1M.
        let dir = tempfile::tempdir().unwrap();
        let tracker = tracker_for(
            dir.path(),
            "s.jsonl",
            &assistant("m1", "claude-haiku-4-5", [250_000, 0, 0, 10]),
        );
        assert_eq!(tracker.snapshot().context_window, Some(ONE_MILLION));
        let snapshot =
            UsageSnapshot { context_tokens: 150, context_window: Some(100), ..Default::default() };
        assert_eq!(snapshot.context_percent(), Some(100));
    }

    #[test]
    fn claude_sent_images_are_labeled() {
        let dir = tempfile::tempdir().unwrap();
        let marker = serde_json::json!({
            "type": "user",
            "message": {"role": "user", "content": [
                {"type": "text", "text": "mira [Image #2] esto"},
                {"type": "image", "source": {"type": "base64", "media_type": "image/png", "data": b64(b"hola")}},
            ]},
        });
        let paste = serde_json::json!({
            "type": "user",
            "imagePasteIds": [5],
            "message": {"role": "user", "content": [
                {"type": "image", "source": {"type": "base64", "media_type": "image/jpeg", "data": b64(b"jpg")}},
                {"type": "text", "text": "sin marcador"},
            ]},
        });
        let tracker = tracker_for(dir.path(), "s.jsonl", &format!("{marker}\n{paste}\n"));
        let images = tracker.images();
        assert_eq!(images.len(), 2);
        assert_eq!(images[0].origin, ImageOrigin::Sent);
        assert_eq!(images[0].label.as_deref(), Some("Image #2"));
        assert_eq!(images[0].media_type, "image/png");
        assert_eq!(images[0].bytes.as_slice(), b"hola");
        assert_eq!(images[1].label.as_deref(), Some("Image #5"));
    }

    #[test]
    fn claude_read_image_labeled_with_path() {
        let dir = tempfile::tempdir().unwrap();
        let tool_use = serde_json::json!({
            "type": "assistant",
            "message": {"id": "m1", "model": "claude-opus-5", "content": [
                {"type": "tool_use", "id": "tu1", "name": "Read", "input": {"file_path": "C:\\img\\a.png"}},
            ], "usage": {"input_tokens": 1, "output_tokens": 1}},
        });
        let result = serde_json::json!({
            "type": "user",
            "message": {"role": "user", "content": [
                {"type": "tool_result", "tool_use_id": "tu1", "content": [
                    {"type": "image", "source": {"type": "base64", "media_type": "image/png", "data": b64(b"png")}},
                ]},
            ]},
        });
        let tracker = tracker_for(dir.path(), "s.jsonl", &format!("{tool_use}\n{result}\n"));
        let images = tracker.images();
        assert_eq!(images.len(), 1);
        assert_eq!(images[0].origin, ImageOrigin::Read);
        assert_eq!(images[0].label.as_deref(), Some("C:\\img\\a.png"));
    }

    #[test]
    fn incremental_poll_waits_for_complete_line() {
        let dir = tempfile::tempdir().unwrap();
        let second = assistant("m2", "claude-opus-5-5", [0, 0, 0, 7]);
        let (head, tail) = second.split_at(20);
        let mut tracker = tracker_for(
            dir.path(),
            "s.jsonl",
            &(assistant("m1", "claude-opus-5-5", [0, 0, 0, 3]) + head),
        );
        assert_eq!(tracker.snapshot().output_tokens, 3);
        append(tracker.path(), tail);
        tracker.poll().unwrap();
        assert_eq!(tracker.snapshot().output_tokens, 10);
        tracker.poll().unwrap();
        assert_eq!(tracker.snapshot().output_tokens, 10);
    }

    #[test]
    fn truncated_file_resets() {
        let dir = tempfile::tempdir().unwrap();
        let text = assistant("m1", "claude-opus-5-5", [0, 0, 0, 3])
            + &assistant("m2", "claude-opus-5-5", [0, 0, 0, 4]);
        let mut tracker = tracker_for(dir.path(), "s.jsonl", &text);
        assert_eq!(tracker.snapshot().output_tokens, 7);
        write(tracker.path(), &assistant("m9", "claude-opus-5-5", [0, 0, 0, 2]));
        tracker.poll().unwrap();
        assert_eq!(tracker.snapshot().output_tokens, 2);
    }

    #[test]
    fn codex_token_count_model_and_images() {
        let dir = tempfile::tempdir().unwrap();
        let codex_dir = dir.path().join(".codex");
        std::fs::create_dir(&codex_dir).unwrap();
        let lines = [
            serde_json::json!({"type": "session_meta", "payload": {"cwd": "C:\\"}}),
            serde_json::json!({"type": "turn_context", "payload": {"model": "gpt-5.4", "cwd": "C:\\"}}),
            serde_json::json!({"type": "event_msg", "payload": {"type": "token_count", "info": null}}),
            serde_json::json!({"type": "response_item", "payload": {"type": "message", "role": "user", "content": [
                {"type": "input_text", "text": "[Image #1]"},
                {"type": "input_image", "image_url": format!("data:image/png;base64,{}", b64(b"cdx"))},
                {"type": "input_text", "text": "</image>"},
            ]}}),
            serde_json::json!({"type": "event_msg", "payload": {"type": "token_count", "info": {
                "total_token_usage": {"input_tokens": 1000, "cached_input_tokens": 600, "output_tokens": 50,
                    "reasoning_output_tokens": 20, "total_tokens": 1050},
                "last_token_usage": {"input_tokens": 800, "cached_input_tokens": 600, "output_tokens": 30,
                    "reasoning_output_tokens": 10, "total_tokens": 830},
                "model_context_window": 258400,
            }}}),
        ];
        let text: String = lines.iter().map(|line| format!("{line}\n")).collect();
        let tracker = tracker_for(&codex_dir, "rollout-x.jsonl", &text);
        let s = tracker.snapshot();
        assert_eq!(tracker.format, TranscriptFormat::Codex);
        assert_eq!(s.model.as_deref(), Some("gpt-5.4"));
        assert_eq!((s.input_tokens, s.cache_read_tokens, s.output_tokens), (400, 600, 50));
        assert_eq!(s.context_tokens, 830);
        assert_eq!(s.context_window, Some(258_400));
        assert_eq!(s.context_percent(), Some(0));
        assert_eq!(s.cost_usd, None);
        let images = tracker.images();
        assert_eq!(images.len(), 1);
        assert_eq!(images[0].label.as_deref(), Some("Image #1"));
        assert_eq!(images[0].bytes.as_slice(), b"cdx");
    }

    #[test]
    fn image_list_is_capped() {
        let dir = tempfile::tempdir().unwrap();
        let line = serde_json::json!({
            "type": "user",
            "message": {"role": "user", "content": [
                {"type": "image", "source": {"type": "base64", "media_type": "image/png", "data": b64(b"x")}},
            ]},
        });
        let text = format!("{line}\n").repeat(MAX_IMAGES + 5);
        let tracker = tracker_for(dir.path(), "s.jsonl", &text);
        assert_eq!(tracker.images().len(), MAX_IMAGES);
    }
}
