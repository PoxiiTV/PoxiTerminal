//! Hilo en segundo plano del módulo Pool.
//!
//! Cada 2 s: lee los payloads de la statusline (muestras de cuota), lee lo nuevo de los transcripts
//! (eventos de uso), y cada ~15 s los envía al servidor; cada ~30 s pide el estado y lo deja en
//! `line.txt` para la statusline y en [`status`] para el panel.
//!
//! Durabilidad: los offsets de los transcripts solo se guardan cuando el servidor ha aceptado todo
//! lo leído hasta ahí, así que ante un corte se releen y se reenvían (el servidor deduplica).
//! ponytail: las muestras de cuota pendientes viven en memoria; si la app se cierra offline se
//! pierden las intermedias (los eventos no). Persistirlas si hiciera falta más granularidad.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard, OnceLock};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use super::client::{Client, Error};
use super::samples::{self, Sample};
use super::state::{ServerState, format_line};
use super::transcript::{self, UsageEvent};
use super::{config, install};

const TICK: Duration = Duration::from_secs(2);
const FLUSH_EVERY: Duration = Duration::from_secs(15);
const STATE_EVERY: Duration = Duration::from_secs(30);
const INSTALL_EVERY: Duration = Duration::from_secs(60);
const SWEEP_EVERY: Duration = Duration::from_secs(30);
const KEEP_MS: i64 = 7 * 24 * 3_600_000;
const SAMPLE_MAX_AGE_MS: i64 = 24 * 3_600_000;
const MAX_SAMPLES_PER_BATCH: usize = 200;
const MAX_EVENTS_PER_BATCH: usize = 500;
const MAX_BUFFERED_SAMPLES: usize = 5_000;
const MAX_BUFFERED_EVENTS: usize = 100_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Phase {
    #[default]
    Off,
    HelperMissing,
    NoToken,
    Connecting,
    Online,
    Offline,
    Unauthorized,
}

#[derive(Debug, Clone, Default)]
pub struct PoolStatus {
    pub phase: Phase,
    pub detail: Option<String>,
    pub last_sync_ms: Option<i64>,
    pub unsent_events: usize,
    pub state: Option<ServerState>,
}

static STATUS: OnceLock<Mutex<PoolStatus>> = OnceLock::new();

fn cell() -> MutexGuard<'static, PoolStatus> {
    STATUS.get_or_init(Default::default).lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// Estado actual para el panel. Barato: una copia bajo un mutex.
pub fn status() -> PoolStatus {
    cell().clone()
}

pub fn spawn() {
    let _ = std::thread::Builder::new().name("poxiterminal-pool".into()).spawn(|| {
        let mut engine = Engine::new(config::pool_dir());
        loop {
            let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| engine.tick()));
            std::thread::sleep(TICK);
        }
    });
}

/// Resultado de [`sync_once`].
pub struct SyncReport {
    pub phase: Phase,
    pub detail: Option<String>,
    pub sent_samples: usize,
    pub sent_events: usize,
    /// La línea de estado que verá la statusline, si el servidor respondió.
    pub line: Option<String>,
}

/// Un ciclo completo y síncrono (`poxiterminal pool sync`): lee lo nuevo, lo envía y pide el estado.
/// Sirve para diagnosticar sin abrir la ventana. No instala la statusline.
pub fn sync_once() -> SyncReport {
    let mut engine = Engine::new(config::pool_dir());
    engine.tick_with(false);
    SyncReport {
        phase: engine.phase,
        detail: engine.detail.clone(),
        sent_samples: engine.sent_samples,
        sent_events: engine.sent_events,
        line: engine.state.as_ref().map(format_line),
    }
}

pub(super) fn now_ms() -> i64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_millis() as i64)
}

fn to_ms(time: SystemTime) -> i64 {
    time.duration_since(UNIX_EPOCH).map_or(0, |d| d.as_millis() as i64)
}

pub(super) struct Engine {
    dir: PathBuf,
    projects: Option<PathBuf>,
    client: Option<(Client, String)>,
    seen: HashMap<PathBuf, SystemTime>,
    read_offsets: HashMap<PathBuf, u64>,
    committed: HashMap<PathBuf, u64>,
    dirty: HashSet<PathBuf>,
    samples: Vec<Sample>,
    events: Vec<UsageEvent>,
    last_flush: Option<Instant>,
    last_state: Option<Instant>,
    last_install: Option<Instant>,
    last_sweep: Option<Instant>,
    last_line: String,
    was_on: bool,
    phase: Phase,
    detail: Option<String>,
    state: Option<ServerState>,
    last_sync_ms: Option<i64>,
    sent_samples: usize,
    sent_events: usize,
}

impl Engine {
    pub(super) fn new(dir: PathBuf) -> Self {
        let committed = load_offsets(&dir);
        Self {
            projects: install::claude_dir().map(|claude| claude.join("projects")),
            read_offsets: committed.clone(),
            committed,
            dir,
            client: None,
            seen: HashMap::new(),
            dirty: HashSet::new(),
            samples: Vec::new(),
            events: Vec::new(),
            last_flush: None,
            last_state: None,
            last_install: None,
            last_sweep: None,
            last_line: String::new(),
            was_on: false,
            phase: Phase::Off,
            detail: None,
            state: None,
            last_sync_ms: None,
            sent_samples: 0,
            sent_events: 0,
        }
    }

    fn tick(&mut self) {
        self.tick_with(true);
    }

    /// Un ciclo completo. `install = false` no toca `settings.json` (diagnóstico).
    fn tick_with(&mut self, install: bool) {
        let cfg = config::load();
        if !cfg.enabled {
            self.turn_off();
            return;
        }
        let Some(server) = cfg.server else {
            self.fail(Phase::NoToken, "falta la dirección del servidor (pool setup)");
            return;
        };
        let Some(token) = config::token() else {
            self.fail(Phase::NoToken, "falta el token (pool setup)");
            return;
        };
        self.was_on = true;
        self.ensure_client(&server, &token);
        if install {
            self.maybe_install();
        }
        let now = now_ms();
        self.collect(now);
        self.flush(now);
        self.refresh_state();
        self.publish();
    }

    fn fail(&mut self, phase: Phase, detail: &str) {
        self.phase = phase;
        self.detail = Some(detail.to_owned());
        self.client = None;
        self.publish();
    }

    /// El usuario ha desactivado el módulo: se restaura la statusline una sola vez.
    fn turn_off(&mut self) {
        if self.was_on {
            self.was_on = false;
            if let Some(claude) = install::claude_dir() {
                let _ = install::apply_at(&claude.join("settings.json"), &self.dir, false, None);
            }
            self.last_line.clear();
        }
        self.client = None;
        self.phase = Phase::Off;
        self.detail = None;
        self.state = None;
        self.publish();
    }

    fn ensure_client(&mut self, server: &str, token: &str) {
        let key = format!("{server}\n{token}");
        if self.client.as_ref().is_none_or(|(_, current)| *current != key) {
            self.client = Some((Client::new(server, token), key));
            self.phase = Phase::Connecting;
            self.detail = None;
            self.last_state = None;
        }
    }

    fn maybe_install(&mut self) {
        if self.last_install.is_some_and(|at| at.elapsed() < INSTALL_EVERY) {
            return;
        }
        self.last_install = Some(Instant::now());
        let Some(claude) = install::claude_dir().filter(|dir| dir.exists()) else { return };
        let helper = crate::ai_hook::helper_command();
        if helper.is_none() {
            self.phase = Phase::HelperMissing;
            self.detail = Some("no se encuentra poxiterminal-hook".into());
            return;
        }
        if let Err(error) =
            install::apply_at(&claude.join("settings.json"), &self.dir, true, helper.as_deref())
        {
            log::warn!("pool: statusline no instalada: {error}");
        }
    }

    /// Lee los payloads de la statusline (muestras) y lo nuevo de los transcripts (eventos).
    pub(super) fn collect(&mut self, now: i64) {
        self.scan_statusline_files(now);
        let sweep = self.last_sweep.is_none_or(|at| at.elapsed() >= SWEEP_EVERY);
        if sweep {
            self.last_sweep = Some(Instant::now());
        }
        let mut targets: Vec<PathBuf> = if sweep {
            self.read_offsets.keys().cloned().collect()
        } else {
            self.dirty.iter().cloned().collect()
        };
        self.dirty.clear();
        // Los subagentes de una sesión viven en `<sesión>/subagents/agent-*.jsonl` y también gastan
        // cuota: se registran junto a su transcript principal.
        for main in targets.clone() {
            for sub in subagent_files(&main) {
                if self.transcript_allowed(&sub) {
                    let start = self.committed.get(&sub).copied().unwrap_or(0);
                    self.read_offsets.entry(sub.clone()).or_insert(start);
                    if !targets.contains(&sub) {
                        targets.push(sub);
                    }
                }
            }
        }
        for path in targets {
            let from = self.read_offsets.get(&path).copied().unwrap_or(0);
            match transcript::read_new(&path, from, now - KEEP_MS) {
                Ok((events, offset)) => {
                    self.read_offsets.insert(path, offset);
                    self.events.extend(events);
                },
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                    self.read_offsets.remove(&path);
                    self.committed.remove(&path);
                },
                Err(_) => {},
            }
        }
        let excess = self.samples.len().saturating_sub(MAX_BUFFERED_SAMPLES);
        self.samples.drain(..excess);
        let excess = self.events.len().saturating_sub(MAX_BUFFERED_EVENTS);
        self.events.drain(..excess);
    }

    fn scan_statusline_files(&mut self, now: i64) {
        let Ok(entries) = std::fs::read_dir(self.dir.join("statusline")) else { return };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().is_none_or(|ext| ext != "json") {
                continue;
            }
            let Some(mtime) = entry.metadata().ok().and_then(|meta| meta.modified().ok()) else {
                continue;
            };
            let age = now - to_ms(mtime);
            if age > KEEP_MS {
                let _ = std::fs::remove_file(&path);
                self.seen.remove(&path);
                continue;
            }
            if self.seen.get(&path) == Some(&mtime) {
                continue;
            }
            self.seen.insert(path.clone(), mtime);
            let Some(parsed) = std::fs::read_to_string(&path)
                .ok()
                .and_then(|raw| samples::parse(&raw, to_ms(mtime)))
            else {
                continue;
            };
            if let Some(sample) = parsed.sample.filter(|_| age <= SAMPLE_MAX_AGE_MS) {
                self.samples.push(sample);
            }
            if let Some(transcript) = parsed.transcript.filter(|t| self.transcript_allowed(t)) {
                let start = self.committed.get(&transcript).copied().unwrap_or(0);
                self.read_offsets.entry(transcript.clone()).or_insert(start);
                self.dirty.insert(transcript);
            }
        }
    }

    /// Solo se leen archivos `.jsonl` dentro de `~/.claude/projects`: el payload es JSON local y no
    /// debe poder apuntar el lector a cualquier ruta.
    fn transcript_allowed(&self, path: &Path) -> bool {
        let Some(projects) = &self.projects else { return false };
        path.extension().is_some_and(|ext| ext == "jsonl")
            && match (path.canonicalize(), projects.canonicalize()) {
                (Ok(path), Ok(projects)) => path.starts_with(projects),
                _ => false,
            }
    }

    /// Envía lo pendiente por lotes. Los offsets solo se confirman si queda todo entregado.
    pub(super) fn flush(&mut self, now: i64) {
        let due = self.last_flush.is_none_or(|at| at.elapsed() >= FLUSH_EVERY);
        if !due && self.events.len() < 400 {
            return;
        }
        let Some((client, _)) = &self.client else { return };
        while !self.samples.is_empty() || !self.events.is_empty() {
            let samples = self.samples.len().min(MAX_SAMPLES_PER_BATCH);
            let events = self.events.len().min(MAX_EVENTS_PER_BATCH);
            match client.ingest(now_ms(), &self.samples[..samples], &self.events[..events]) {
                Ok(()) => {
                    self.sent_samples += samples;
                    self.sent_events += events;
                },
                // Un lote que el servidor considera inválido se descarta: reintentarlo
                // eternamente bloquearía todo lo que viene detrás.
                Err(Error::Rejected(status)) if status < 500 && status != 429 => {
                    log::warn!("pool: lote rechazado ({status}); descartado");
                },
                Err(Error::Unauthorized) => {
                    self.phase = Phase::Unauthorized;
                    self.detail = Some(Error::Unauthorized.to_string());
                    return;
                },
                Err(error) => {
                    self.phase = Phase::Offline;
                    self.detail = Some(error.to_string());
                    return;
                },
            }
            self.samples.drain(..samples);
            self.events.drain(..events);
        }
        self.last_flush = Some(Instant::now());
        self.last_sync_ms = Some(now);
        if self.committed != self.read_offsets {
            self.committed = self.read_offsets.clone();
            save_offsets(&self.dir, &self.committed);
        }
        self.phase = Phase::Online;
        self.detail = None;
    }

    fn refresh_state(&mut self) {
        let due = self.last_state.is_none_or(|at| at.elapsed() >= STATE_EVERY);
        let Some((client, _)) = self.client.as_ref().filter(|_| due) else { return };
        self.last_state = Some(Instant::now());
        match client.state() {
            Ok(state) => {
                self.write_line(&format_line(&state));
                self.state = Some(state);
                self.phase = Phase::Online;
                self.detail = None;
                self.last_sync_ms = Some(now_ms());
            },
            Err(Error::Unauthorized) => {
                self.phase = Phase::Unauthorized;
                self.detail = Some(Error::Unauthorized.to_string());
                self.write_line("Pool · token rechazado");
            },
            Err(error) => {
                self.phase = Phase::Offline;
                self.detail = Some(error.to_string());
                self.write_line("Pool · sin conexión");
            },
        }
    }

    fn write_line(&mut self, text: &str) {
        if self.last_line != text {
            let _ = std::fs::create_dir_all(&self.dir);
            let _ = crate::atomic_file::write(&self.dir.join("line.txt"), text.as_bytes());
            self.last_line = text.to_owned();
        }
    }

    fn publish(&self) {
        let mut shared = cell();
        shared.phase = self.phase;
        shared.detail = self.detail.clone();
        shared.last_sync_ms = self.last_sync_ms;
        shared.unsent_events = self.events.len();
        shared.state = self.state.clone();
    }
}

/// Transcripts de los subagentes de `<sesión>.jsonl`: `<sesión>/subagents/*.jsonl`.
fn subagent_files(main: &Path) -> Vec<PathBuf> {
    let Ok(entries) = std::fs::read_dir(main.with_extension("").join("subagents")) else {
        return Vec::new();
    };
    entries
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| path.extension().is_some_and(|ext| ext == "jsonl"))
        .collect()
}

fn offsets_path(dir: &Path) -> PathBuf {
    dir.join("offsets.json")
}

fn load_offsets(dir: &Path) -> HashMap<PathBuf, u64> {
    std::fs::read_to_string(offsets_path(dir))
        .ok()
        .and_then(|raw| serde_json::from_str::<HashMap<String, u64>>(&raw).ok())
        .map(|map| map.into_iter().map(|(path, offset)| (PathBuf::from(path), offset)).collect())
        .unwrap_or_default()
}

fn save_offsets(dir: &Path, offsets: &HashMap<PathBuf, u64>) {
    let map: HashMap<String, u64> = offsets
        .iter()
        .map(|(path, offset)| (path.to_string_lossy().into_owned(), *offset))
        .collect();
    if let Ok(raw) = serde_json::to_vec(&map) {
        let _ = std::fs::create_dir_all(dir);
        let _ = crate::atomic_file::write(&offsets_path(dir), &raw);
    }
}

#[cfg(test)]
mod tests {
    use std::io::{Read, Write};
    use std::net::TcpListener;
    use std::sync::Arc;

    use serde_json::Value;

    use super::*;

    /// Servidor falso que acepta cualquier número de peticiones y las guarda. `status` es la línea
    /// de estado con la que contesta.
    fn mock(status: &'static str) -> (String, Arc<Mutex<Vec<String>>>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let seen = Arc::new(Mutex::new(Vec::new()));
        let log = seen.clone();
        std::thread::spawn(move || {
            for stream in listener.incoming() {
                let Ok(mut stream) = stream else { break };
                let mut request = Vec::new();
                let mut buffer = [0u8; 8192];
                loop {
                    let Ok(read) = stream.read(&mut buffer) else { break };
                    request.extend_from_slice(&buffer[..read]);
                    let text = String::from_utf8_lossy(&request).into_owned();
                    let Some(head) = text.find("\r\n\r\n") else { continue };
                    let length = text
                        .lines()
                        .find_map(|l| {
                            l.to_ascii_lowercase()
                                .strip_prefix("content-length:")
                                .and_then(|v| v.trim().parse::<usize>().ok())
                        })
                        .unwrap_or(0);
                    if request.len() >= head + 4 + length {
                        break;
                    }
                }
                log.lock().unwrap().push(String::from_utf8_lossy(&request).into_owned());
                let body = "{}";
                let _ = write!(
                    stream,
                    "HTTP/1.1 {status}\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
                    body.len()
                );
            }
        });
        (url, seen)
    }

    struct Fixture {
        root: PathBuf,
        engine: Engine,
    }

    fn fixture(name: &str, server: &str) -> Fixture {
        let root = std::env::temp_dir().join(format!("pool-engine-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let projects = root.join("claude").join("projects").join("p");
        std::fs::create_dir_all(&projects).unwrap();
        std::fs::create_dir_all(root.join("pool").join("statusline")).unwrap();
        let mut engine = Engine::new(root.join("pool"));
        engine.projects = Some(root.join("claude").join("projects"));
        engine.client = Some((Client::new(server, "cmk_test"), "k".into()));
        Fixture { root, engine }
    }

    fn assistant(id: &str, output: u64) -> String {
        format!(
            r#"{{"type":"assistant","sessionId":"s1","requestId":"req_{id}","timestamp":"{}","message":{{"id":"{id}","model":"claude-sonnet-5-5","usage":{{"input_tokens":2,"cache_creation_input_tokens":10,"cache_read_input_tokens":20,"output_tokens":{output}}}}}}}"#,
            chrono::Utc::now().to_rfc3339()
        )
    }

    fn ingest_bodies(seen: &Arc<Mutex<Vec<String>>>) -> Vec<Value> {
        seen.lock()
            .unwrap()
            .iter()
            .filter(|request| request.starts_with("POST /api/v1/ingest"))
            .map(|request| serde_json::from_str(request.split("\r\n\r\n").nth(1).unwrap()).unwrap())
            .collect()
    }

    fn write_statusline(fx: &Fixture, transcript: &Path) {
        let payload = format!(
            r#"{{"session_id":"s1","transcript_path":{},"rate_limits":{{"five_hour":{{"used_percentage":21,"resets_at":1792012200}}}}}}"#,
            serde_json::to_string(&transcript.to_string_lossy()).unwrap()
        );
        std::fs::write(fx.root.join("pool").join("statusline").join("s1.json"), payload).unwrap();
    }

    #[test]
    fn samples_and_transcript_events_reach_the_server_and_offsets_are_committed_once() {
        let (url, seen) = mock("200 OK");
        let mut fx = fixture("flow", &url);
        let transcript = fx.root.join("claude").join("projects").join("p").join("s1.jsonl");
        std::fs::write(&transcript, format!("{}\n{}\n", assistant("m1", 5), assistant("m1", 9)))
            .unwrap();
        write_statusline(&fx, &transcript);

        let now = now_ms();
        fx.engine.collect(now);
        fx.engine.flush(now);

        let bodies = ingest_bodies(&seen);
        assert_eq!(bodies.len(), 1);
        assert_eq!(bodies[0]["samples"][0]["five_hour"]["used_percentage"], 21);
        assert_eq!(bodies[0]["events"].as_array().unwrap().len(), 1, "las dos líneas se funden");
        assert_eq!(bodies[0]["events"][0]["output_tokens"], 9);
        assert!(!bodies[0].to_string().contains("transcript_path"), "no se envía la ruta");

        // offsets guardados; un reinicio no vuelve a leer ni reenviar lo ya entregado
        let reloaded = Engine::new(fx.root.join("pool"));
        assert_eq!(
            reloaded.committed.get(&transcript).copied(),
            Some(std::fs::metadata(&transcript).unwrap().len())
        );

        // nada nuevo → nada que enviar
        fx.engine.last_flush = None;
        fx.engine.collect(now_ms());
        fx.engine.flush(now_ms());
        assert_eq!(ingest_bodies(&seen).len(), 1);
    }

    #[test]
    fn only_new_transcript_lines_are_sent_on_the_next_round() {
        let (url, seen) = mock("200 OK");
        let mut fx = fixture("incremental", &url);
        let transcript = fx.root.join("claude").join("projects").join("p").join("s1.jsonl");
        std::fs::write(&transcript, format!("{}\n", assistant("m1", 1))).unwrap();
        write_statusline(&fx, &transcript);
        fx.engine.collect(now_ms());
        fx.engine.flush(now_ms());

        let mut file = std::fs::OpenOptions::new().append(true).open(&transcript).unwrap();
        writeln!(file, "{}", assistant("m2", 2)).unwrap();
        fx.engine.last_flush = None;
        fx.engine.last_sweep = None;
        fx.engine.collect(now_ms());
        fx.engine.flush(now_ms());

        let bodies = ingest_bodies(&seen);
        assert_eq!(bodies.len(), 2);
        let ids: Vec<&str> = bodies[1]["events"]
            .as_array()
            .unwrap()
            .iter()
            .map(|e| e["message_id"].as_str().unwrap())
            .collect();
        assert_eq!(ids, ["m2"]);
    }

    #[test]
    fn when_the_server_is_unreachable_nothing_is_lost_or_committed() {
        let closed = TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}", closed.local_addr().unwrap());
        drop(closed);
        let mut fx = fixture("offline", &url);
        let transcript = fx.root.join("claude").join("projects").join("p").join("s1.jsonl");
        std::fs::write(&transcript, format!("{}\n", assistant("m1", 1))).unwrap();
        write_statusline(&fx, &transcript);

        fx.engine.collect(now_ms());
        fx.engine.flush(now_ms());

        assert_eq!(fx.engine.phase, Phase::Offline);
        assert_eq!(fx.engine.events.len(), 1);
        assert_eq!(fx.engine.samples.len(), 1);
        assert!(fx.engine.committed.is_empty());
        assert!(!offsets_path(&fx.root.join("pool")).exists());
    }

    #[test]
    fn a_revoked_token_is_reported_and_keeps_the_data() {
        let (url, _seen) = mock("401 Unauthorized");
        let mut fx = fixture("unauthorized", &url);
        let transcript = fx.root.join("claude").join("projects").join("p").join("s1.jsonl");
        std::fs::write(&transcript, format!("{}\n", assistant("m1", 1))).unwrap();
        write_statusline(&fx, &transcript);
        fx.engine.collect(now_ms());
        fx.engine.flush(now_ms());
        assert_eq!(fx.engine.phase, Phase::Unauthorized);
        assert_eq!(fx.engine.events.len(), 1);
    }

    #[test]
    fn subagent_transcripts_are_read_too_and_marked_as_sidechain() {
        let (url, seen) = mock("200 OK");
        let mut fx = fixture("subagents", &url);
        let project = fx.root.join("claude").join("projects").join("p");
        let transcript = project.join("s1.jsonl");
        std::fs::write(&transcript, format!("{}\n", assistant("main1", 1))).unwrap();
        // Claude Code guarda cada subagente en <sesión>/subagents/agent-*.jsonl
        let subagents = project.join("s1").join("subagents");
        std::fs::create_dir_all(&subagents).unwrap();
        let side = assistant("sub1", 7)
            .replace(r#"{"type":"assistant","#, r#"{"type":"assistant","isSidechain":true,"#);
        std::fs::write(subagents.join("agent-abc.jsonl"), format!("{side}\n")).unwrap();
        std::fs::write(subagents.join("notas.txt"), "no es un transcript").unwrap();
        write_statusline(&fx, &transcript);

        fx.engine.collect(now_ms());
        fx.engine.flush(now_ms());

        let bodies = ingest_bodies(&seen);
        let events = bodies[0]["events"].as_array().unwrap();
        let by_id = |id: &str| events.iter().find(|e| e["message_id"] == id).unwrap();
        assert_eq!(events.len(), 2);
        assert_eq!(by_id("main1")["sidechain"], false);
        assert_eq!(by_id("sub1")["sidechain"], true);
        assert_eq!(by_id("sub1")["output_tokens"], 7);

        // un subagente que aparece más tarde también se recoge
        std::fs::write(subagents.join("agent-def.jsonl"), format!("{}\n", assistant("sub2", 3)))
            .unwrap();
        fx.engine.last_flush = None;
        fx.engine.last_sweep = None;
        fx.engine.collect(now_ms());
        fx.engine.flush(now_ms());
        let later = ingest_bodies(&seen);
        let ids: Vec<&str> = later[1]["events"]
            .as_array()
            .unwrap()
            .iter()
            .map(|e| e["message_id"].as_str().unwrap())
            .collect();
        assert_eq!(ids, ["sub2"]);
    }

    #[test]
    fn a_batch_the_server_calls_invalid_is_dropped_instead_of_blocking_the_queue() {
        let (url, _seen) = mock("400 Bad Request");
        let mut fx = fixture("poison", &url);
        let transcript = fx.root.join("claude").join("projects").join("p").join("s1.jsonl");
        std::fs::write(&transcript, format!("{}\n", assistant("m1", 1))).unwrap();
        write_statusline(&fx, &transcript);
        fx.engine.collect(now_ms());
        fx.engine.flush(now_ms());
        assert!(fx.engine.events.is_empty() && fx.engine.samples.is_empty());
    }

    #[test]
    fn transcripts_outside_claude_projects_are_never_read() {
        let (url, seen) = mock("200 OK");
        let mut fx = fixture("outside", &url);
        let outside = fx.root.join("otro.jsonl");
        std::fs::write(&outside, format!("{}\n", assistant("m9", 1))).unwrap();
        write_statusline(&fx, &outside);
        fx.engine.collect(now_ms());
        fx.engine.flush(now_ms());
        let bodies = ingest_bodies(&seen);
        assert!(bodies.iter().all(|body| body["events"].as_array().unwrap().is_empty()));
    }
}
