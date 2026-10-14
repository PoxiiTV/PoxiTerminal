//! `poxiterminal-hook claude-statusline --dir <pool dir>` — el comando de statusline de Claude Code
//! cuando el módulo Pool está activado.
//!
//! Claude lo ejecuta tras cada respuesta con el JSON de estado por stdin y muestra lo que imprima
//! por stdout. Este helper hace tres cosas, todas sin depender de que PoxiTerminal esté abierto:
//!
//! 1. Guarda el payload crudo en `<dir>/statusline/<session_id>.json` (escritura atómica). La app
//!    lo lee cuando está en marcha; no se parsea JSON aquí (el crate es `std` puro).
//! 2. Ejecuta la statusline anterior del usuario si hay una guardada en `<dir>/chain.txt` (Claude
//!    solo admite un comando), pasándole el mismo payload.
//! 3. Imprime la línea de estado que la app deja en `<dir>/line.txt`, seguida de la salida de la
//!    statusline encadenada.
//!
//! Igual que el resto del helper: nunca falla hacia Claude (siempre sale con 0) y está acotado en
//! tiempo.

use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::time::Duration;

const MAX_PAYLOAD_BYTES: usize = 1 << 20;
const MAX_CHAIN_OUTPUT_BYTES: u64 = 64 * 1024;
const MAX_LINE_CHARS: usize = 300;
const TOTAL_TIMEOUT: Duration = Duration::from_secs(4);
const CHAIN_TIMEOUT: Duration = Duration::from_millis(2500);

/// Intérpretes con los que se puede ejecutar la statusline encadenada, por orden de preferencia.
/// Claude Code ejecuta los comandos con bash (Git Bash en Windows), así que el comando previo suele
/// estar escrito para él.
///
/// En Windows NUNCA se prueba `bash` a secas: resuelve a `System32\bash.exe`, el lanzador de WSL, que
/// ejecutaría el comando dentro de Linux (otro `OSTYPE`, otras rutas) y además tarda en arrancar.
fn shells() -> Vec<PathBuf> {
    #[cfg(windows)]
    {
        let mut list = Vec::new();
        // La misma variable que usa Claude Code para localizar Git Bash.
        if let Some(path) = std::env::var_os("CLAUDE_CODE_GIT_BASH_PATH").filter(|p| !p.is_empty()) {
            list.push(PathBuf::from(path));
        }
        for (variable, subdir) in
            [("ProgramFiles", ""), ("ProgramFiles(x86)", ""), ("LOCALAPPDATA", "Programs")]
        {
            if let Some(base) = std::env::var_os(variable) {
                list.push(PathBuf::from(base).join(subdir).join("Git").join("bin").join("bash.exe"));
            }
        }
        list.push(PathBuf::from("sh")); // por si Git\usr\bin está en el PATH
        list
    }
    #[cfg(not(windows))]
    {
        vec![PathBuf::from("sh"), PathBuf::from("bash")]
    }
}

/// Punto de entrada: `args` son los argumentos tras `claude-statusline`.
pub fn run(args: &[String]) {
    let dir = dir_arg(args);
    let (done, finished) = mpsc::sync_channel::<String>(1);
    let spawned = std::thread::Builder::new()
        .name("hook-statusline".into())
        .spawn(move || {
            let output = std::panic::catch_unwind(|| produce(dir.as_deref())).unwrap_or_default();
            let _ = done.send(output);
        })
        .is_ok();
    if spawned && let Ok(output) = finished.recv_timeout(TOTAL_TIMEOUT) && !output.is_empty() {
        let mut stdout = std::io::stdout().lock();
        let _ = stdout.write_all(output.as_bytes());
        let _ = stdout.flush();
    }
}

fn dir_arg(args: &[String]) -> Option<PathBuf> {
    let at = args.iter().position(|arg| arg == "--dir")?;
    args.get(at + 1).filter(|value| !value.is_empty()).map(PathBuf::from)
}

fn produce(dir: Option<&Path>) -> String {
    let payload = read_bounded(std::io::stdin().lock()).unwrap_or_default();
    let Some(dir) = dir else { return String::new() };

    if let Some(session) = std::str::from_utf8(&payload).ok().and_then(session_id) {
        let _ = write_atomic(&dir.join("statusline").join(format!("{session}.json")), &payload);
    }

    let chained = std::fs::read_to_string(dir.join("chain.txt"))
        .ok()
        .map(|text| text.trim().to_owned())
        .filter(|command| !command.is_empty())
        .and_then(|command| run_chain(&command, &payload));
    let line = std::fs::read_to_string(dir.join("line.txt")).ok();
    compose(line.as_deref(), chained.as_deref())
}

fn read_bounded(mut reader: impl Read) -> std::io::Result<Vec<u8>> {
    let mut bytes = Vec::with_capacity(4096);
    reader.by_ref().take((MAX_PAYLOAD_BYTES + 1) as u64).read_to_end(&mut bytes)?;
    if bytes.len() > MAX_PAYLOAD_BYTES {
        std::io::copy(&mut reader, &mut std::io::sink())?;
        bytes.clear();
    }
    Ok(bytes)
}

/// Extrae `session_id` sin parsear JSON. Solo se aceptan ids "seguros" porque el valor acaba siendo
/// un nombre de archivo: letras, dígitos, `-` y `_`, de 1 a 64 caracteres.
pub fn session_id(payload: &str) -> Option<String> {
    let key = payload.find("\"session_id\"")?;
    let rest = payload[key + "\"session_id\"".len()..].trim_start().strip_prefix(':')?;
    let value = rest.trim_start().strip_prefix('"')?;
    let id = &value[..value.find('"')?];
    let valid = !id.is_empty()
        && id.len() <= 64
        && id.bytes().all(|byte| byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'_');
    valid.then(|| id.to_owned())
}

/// Primera línea de `line.txt` (recortada) y a continuación la salida encadenada.
pub fn compose(line: Option<&str>, chained: Option<&str>) -> String {
    let own: String = line
        .and_then(|text| text.trim_start_matches('\u{feff}').lines().next())
        .map(|first| first.trim().chars().take(MAX_LINE_CHARS).collect())
        .unwrap_or_default();
    let chained = chained.map(str::trim_end).filter(|text| !text.is_empty());
    match (own.is_empty(), chained) {
        (true, None) => String::new(),
        (false, None) => format!("{own}\n"),
        (true, Some(other)) => format!("{other}\n"),
        (false, Some(other)) => format!("{own}\n{other}\n"),
    }
}

fn write_atomic(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let tmp = path.with_extension(format!("tmp{}", std::process::id()));
    std::fs::write(&tmp, bytes)?;
    std::fs::rename(&tmp, path).inspect_err(|_| {
        let _ = std::fs::remove_file(&tmp);
    })
}

/// Prueba los intérpretes en orden y se queda con el primero que arranca. Si ese agota el tiempo,
/// no se prueba otro: dejaría un segundo proceso vivo que nadie llegaría a matar.
fn run_chain(command: &str, payload: &[u8]) -> Option<String> {
    for shell in shells() {
        match run_chain_with(&shell, command, payload) {
            Err(Unavailable) => continue,
            Ok(output) => return output,
        }
    }
    None
}

/// El intérprete no existe o no pudo arrancar.
struct Unavailable;

fn run_chain_with(
    shell: &Path,
    command: &str,
    payload: &[u8],
) -> Result<Option<String>, Unavailable> {
    let mut cmd = Command::new(shell);
    cmd.arg("-c").arg(command).stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x0800_0000); // CREATE_NO_WINDOW: sin parpadeo de consola
    }
    let mut child = cmd.spawn().map_err(|_| Unavailable)?;

    if let Some(mut stdin) = child.stdin.take() {
        let data = payload.to_vec();
        std::thread::spawn(move || {
            let _ = stdin.write_all(&data);
        });
    }
    let Some(mut stdout) = child.stdout.take() else {
        kill_tree(&mut child);
        return Ok(None);
    };
    let (sender, receiver) = mpsc::channel();
    std::thread::spawn(move || {
        let mut buffer = Vec::new();
        let _ = (&mut stdout).take(MAX_CHAIN_OUTPUT_BYTES).read_to_end(&mut buffer);
        let _ = sender.send(buffer);
    });
    match receiver.recv_timeout(CHAIN_TIMEOUT) {
        Ok(buffer) => {
            let _ = child.wait();
            Ok(Some(String::from_utf8_lossy(&buffer).into_owned()))
        },
        Err(_) => {
            kill_tree(&mut child);
            Ok(None)
        },
    }
}

/// Mata al hijo y a sus descendientes. En Windows el hijo hereda nuestro stdout, así que un nieto
/// vivo (`sleep` bajo `sh -c`) mantendría abierta la tubería de Claude hasta que termine solo.
fn kill_tree(child: &mut std::process::Child) {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        let _ = Command::new("taskkill")
            .args(["/F", "/T", "/PID", &child.id().to_string()])
            .creation_flags(0x0800_0000)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
    }
    let _ = child.kill();
    let _ = child.wait();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn session_id_is_extracted_without_parsing_json() {
        let payload = r#"{"cwd":"x","session_id":"3546799b-9121-489b-927e-77c5c2368ad6","model":{}}"#;
        assert_eq!(session_id(payload).as_deref(), Some("3546799b-9121-489b-927e-77c5c2368ad6"));
        assert_eq!(session_id(r#"{"session_id" :  "abc_1"}"#).as_deref(), Some("abc_1"));
        assert_eq!(session_id(r#"{"otro":1}"#), None);
        assert_eq!(session_id(r#"{"session_id":null}"#), None);
    }

    #[test]
    fn unsafe_session_ids_never_become_file_names() {
        for hostile in [r#"..\\..\\x"#, "a/b", "a b", "con.punto", "", &"a".repeat(65)] {
            let payload = format!(r#"{{"session_id":"{hostile}"}}"#);
            assert_eq!(session_id(&payload), None, "{hostile:?}");
        }
    }

    #[test]
    fn compose_prints_own_line_then_chained_output() {
        assert_eq!(compose(Some("Pool 1/2\n"), None), "Pool 1/2\n");
        assert_eq!(compose(Some("Pool\nsegunda"), Some("otra\n\n")), "Pool\notra\n");
        assert_eq!(compose(None, Some("solo cadena")), "solo cadena\n");
        assert_eq!(compose(Some("  \n"), None), "");
        assert_eq!(compose(None, None), "");
        assert_eq!(compose(Some("\u{feff}con bom"), None), "con bom\n");
        assert_eq!(compose(Some(&"x".repeat(1000)), None).chars().count(), MAX_LINE_CHARS + 1);
    }

    #[test]
    fn payload_is_saved_atomically_per_session_and_nothing_leaks_out_of_the_dir() {
        let dir = std::env::temp_dir().join(format!("pool-hook-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let payload = br#"{"session_id":"s-1","rate_limits":{"five_hour":{"used_percentage":21}}}"#;
        let out = {
            let path = dir.join("statusline").join("s-1.json");
            write_atomic(&path, payload).unwrap();
            std::fs::read(&path).unwrap()
        };
        assert_eq!(out, payload);
        let leftovers = std::fs::read_dir(dir.join("statusline")).unwrap().count();
        assert_eq!(leftovers, 1, "no debe quedar el temporal");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn oversized_stdin_is_discarded_not_truncated() {
        let big = vec![b'x'; MAX_PAYLOAD_BYTES * 2];
        assert!(read_bounded(&big[..]).unwrap().is_empty());
        assert_eq!(read_bounded(&b"{}"[..]).unwrap(), b"{}");
    }

    #[test]
    fn dir_argument_is_required_and_must_not_be_empty() {
        let args = |list: &[&str]| list.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        assert_eq!(dir_arg(&args(&["--dir", "C:/datos/pool"])), Some(PathBuf::from("C:/datos/pool")));
        assert_eq!(dir_arg(&args(&["--dir"])), None);
        assert_eq!(dir_arg(&args(&["--dir", ""])), None);
        assert_eq!(dir_arg(&args(&[])), None);
    }

    #[test]
    fn chained_command_receives_the_payload_and_its_output_is_captured() {
        // Solo si hay un `sh` utilizable (Git Bash en Windows, cualquiera en Unix).
        let Some(out) = run_chain("cat", b"hola payload") else { return };
        assert_eq!(out, "hola payload");
    }

    /// En Windows `bash` a secas es el lanzador de WSL: ejecutaría la statusline del usuario en Linux.
    #[cfg(windows)]
    #[test]
    fn windows_never_falls_back_to_the_wsl_bash_launcher() {
        assert!(shells().iter().all(|shell| shell != Path::new("bash")));
    }

    #[test]
    fn a_hung_chained_command_is_killed_instead_of_blocking_claude() {
        let started = std::time::Instant::now();
        let out = run_chain("sleep 30", b"");
        assert!(started.elapsed() < Duration::from_secs(8));
        assert!(out.is_none() || out.as_deref() == Some(""));
    }
}
