//! Lector incremental de transcripts JSONL de Claude Code → eventos de uso por mensaje.
//!
//! Independiente del panel Agentes: la statusline nos dice qué transcripts existen y aquí se leen
//! desde un offset guardado. Solo se extraen contadores, modelo, ids y hora; nunca el contenido.

use std::collections::HashMap;
use std::fs::File;
use std::io::{BufRead, BufReader, Seek, SeekFrom};
use std::path::Path;

use serde::Serialize;
use serde_json::Value;

/// Tope que acepta el servidor por campo.
const MAX_TOKENS: u64 = 1_000_000_000;

/// Un mensaje del asistente con su uso. Se serializa con los nombres que espera el servidor.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct UsageEvent {
    pub message_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub request_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
    pub model: String,
    /// Milisegundos del reloj de este dispositivo.
    pub ts: i64,
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cache_read_tokens: u64,
    pub cache_write_tokens: u64,
    pub sidechain: bool,
}

impl UsageEvent {
    fn key(&self) -> (String, String) {
        (self.message_id.clone(), self.request_id.clone().unwrap_or_default())
    }
}

pub fn parse_line(line: &str) -> Option<UsageEvent> {
    // Filtro barato antes de parsear: la mayoría de líneas no son del asistente.
    if !line.contains("\"assistant\"") {
        return None;
    }
    let value: Value = serde_json::from_str(line).ok()?;
    if value["type"].as_str()? != "assistant" {
        return None;
    }
    let message = &value["message"];
    let usage = message.get("usage")?;
    let model = message["model"].as_str().filter(|m| !m.is_empty() && *m != "<synthetic>")?;
    let message_id = message["id"].as_str().filter(|id| !id.is_empty() && id.len() <= 100)?;
    let ts =
        chrono::DateTime::parse_from_rfc3339(value["timestamp"].as_str()?).ok()?.timestamp_millis();
    let tokens = |field: &str| usage[field].as_u64().unwrap_or(0).min(MAX_TOKENS);
    let short = |field: &str| {
        value[field]
            .as_str()
            .filter(|text| !text.is_empty() && text.len() <= 100)
            .map(str::to_owned)
    };
    Some(UsageEvent {
        message_id: message_id.to_owned(),
        request_id: short("requestId"),
        session_id: short("sessionId"),
        model: model.chars().take(100).collect(),
        ts,
        input_tokens: tokens("input_tokens"),
        output_tokens: tokens("output_tokens"),
        cache_read_tokens: tokens("cache_read_input_tokens"),
        cache_write_tokens: tokens("cache_creation_input_tokens"),
        sidechain: value["isSidechain"].as_bool().unwrap_or(false),
    })
}

/// Claude escribe varias líneas por mensaje con uso creciente: se queda una por mensaje con el
/// máximo de cada campo (el servidor hace lo mismo, esto solo reduce el lote).
fn merge(events: Vec<UsageEvent>) -> Vec<UsageEvent> {
    let mut order: Vec<(String, String)> = Vec::new();
    let mut merged: HashMap<(String, String), UsageEvent> = HashMap::new();
    for event in events {
        let key = event.key();
        match merged.get_mut(&key) {
            Some(existing) => {
                existing.input_tokens = existing.input_tokens.max(event.input_tokens);
                existing.output_tokens = existing.output_tokens.max(event.output_tokens);
                existing.cache_read_tokens =
                    existing.cache_read_tokens.max(event.cache_read_tokens);
                existing.cache_write_tokens =
                    existing.cache_write_tokens.max(event.cache_write_tokens);
            },
            None => {
                order.push(key.clone());
                merged.insert(key, event);
            },
        }
    }
    order.into_iter().filter_map(|key| merged.remove(&key)).collect()
}

/// Lee lo nuevo desde `from`. Devuelve los eventos (con `ts >= min_ts`) y el offset hasta el que se
/// ha consumido: solo líneas completas, de modo que una línea a medias se relee la próxima vez.
/// Si el archivo se ha encogido (reescrito), se empieza de cero.
pub fn read_new(path: &Path, from: u64, min_ts: i64) -> std::io::Result<(Vec<UsageEvent>, u64)> {
    let mut file = File::open(path)?;
    let len = file.metadata()?.len();
    let start = if len < from { 0 } else { from };
    if start == len {
        return Ok((Vec::new(), start));
    }
    file.seek(SeekFrom::Start(start))?;
    let mut reader = BufReader::new(file);
    let mut offset = start;
    let mut events = Vec::new();
    let mut line = String::new();
    loop {
        line.clear();
        let read = reader.read_line(&mut line)?;
        if read == 0 || !line.ends_with('\n') {
            break; // fin de archivo o línea todavía incompleta
        }
        offset += read as u64;
        if let Some(event) = parse_line(&line)
            && event.ts >= min_ts
        {
            events.push(event);
        }
    }
    Ok((merge(events), offset))
}

#[cfg(test)]
mod tests {
    use std::io::Write;

    use super::*;

    fn line(id: &str, request: &str, output: u64, ts: &str) -> String {
        format!(
            r#"{{"type":"assistant","isSidechain":false,"sessionId":"s1","requestId":"{request}","timestamp":"{ts}","message":{{"id":"{id}","model":"claude-sonnet-5-5","content":[{{"type":"text","text":"SECRETO"}}],"usage":{{"input_tokens":2,"cache_creation_input_tokens":100,"cache_read_input_tokens":300,"output_tokens":{output}}}}}}}"#
        )
    }

    fn scratch(name: &str) -> std::path::PathBuf {
        let dir =
            std::env::temp_dir().join(format!("pool-transcript-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir.join("t.jsonl")
    }

    #[test]
    fn an_assistant_line_becomes_an_event_without_any_content() {
        let event = parse_line(&line("m1", "r1", 50, "2026-10-14T18:47:49.768Z")).unwrap();
        assert_eq!(event.message_id, "m1");
        assert_eq!(event.request_id.as_deref(), Some("r1"));
        assert_eq!(event.session_id.as_deref(), Some("s1"));
        assert_eq!(event.model, "claude-sonnet-5-5");
        assert_eq!((event.input_tokens, event.output_tokens), (2, 50));
        assert_eq!((event.cache_read_tokens, event.cache_write_tokens), (300, 100));
        assert_eq!(event.ts, 1_792_003_669_768);
        assert!(!serde_json::to_string(&event).unwrap().contains("SECRETO"));
    }

    #[test]
    fn other_lines_synthetic_models_and_missing_fields_are_skipped() {
        assert!(parse_line(r#"{"type":"user","message":{"content":"hola"}}"#).is_none());
        assert!(parse_line("no es json con \"assistant\"").is_none());
        let synthetic =
            line("m", "r", 1, "2026-10-14T18:00:00Z").replace("claude-sonnet-5-5", "<synthetic>");
        assert!(parse_line(&synthetic).is_none());
        let no_time =
            line("m", "r", 1, "2026-10-14T18:00:00Z").replace("\"timestamp\"", "\"otro\"");
        assert!(parse_line(&no_time).is_none());
    }

    #[test]
    fn lines_of_the_same_message_merge_to_the_max_usage() {
        let path = scratch("merge");
        let mut file = File::create(&path).unwrap();
        for output in [10, 50, 30] {
            writeln!(file, "{}", line("m1", "r1", output, "2026-10-14T18:00:00Z")).unwrap();
        }
        writeln!(file, "{}", line("m2", "r2", 7, "2026-10-14T18:00:01Z")).unwrap();
        let (events, offset) = read_new(&path, 0, 0).unwrap();
        assert_eq!(events.len(), 2);
        assert_eq!(events[0].output_tokens, 50);
        assert_eq!(events[1].output_tokens, 7);
        assert_eq!(offset, std::fs::metadata(&path).unwrap().len());
    }

    #[test]
    fn incremental_reads_pick_up_only_new_lines_and_wait_for_a_partial_one() {
        let path = scratch("incremental");
        let mut file = File::create(&path).unwrap();
        writeln!(file, "{}", line("m1", "r1", 1, "2026-10-14T18:00:00Z")).unwrap();
        let (first, offset) = read_new(&path, 0, 0).unwrap();
        assert_eq!(first.len(), 1);

        // media línea: no se consume
        let full = line("m2", "r2", 2, "2026-10-14T18:00:05Z");
        let (head, tail) = full.split_at(full.len() / 2);
        write!(file, "{head}").unwrap();
        let (none, same) = read_new(&path, offset, 0).unwrap();
        assert!(none.is_empty());
        assert_eq!(same, offset);

        writeln!(file, "{tail}").unwrap();
        let (second, end) = read_new(&path, offset, 0).unwrap();
        assert_eq!(second.len(), 1);
        assert_eq!(second[0].message_id, "m2");
        assert!(read_new(&path, end, 0).unwrap().0.is_empty());
    }

    #[test]
    fn old_events_are_filtered_and_a_shrunken_file_restarts_from_zero() {
        let path = scratch("old");
        let mut file = File::create(&path).unwrap();
        writeln!(file, "{}", line("old", "r", 1, "2020-01-01T00:00:00Z")).unwrap();
        writeln!(file, "{}", line("new", "r", 1, "2026-10-14T18:00:00Z")).unwrap();
        let (events, _) = read_new(&path, 0, 1_700_000_000_000).unwrap();
        assert_eq!(events.iter().map(|e| e.message_id.as_str()).collect::<Vec<_>>(), ["new"]);
        // offset mayor que el archivo = reescrito
        let (again, _) = read_new(&path, 10_000_000, 1_700_000_000_000).unwrap();
        assert_eq!(again.len(), 1);
    }
}
