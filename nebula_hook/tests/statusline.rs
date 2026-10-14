//! `claude-statusline` contra el binario real: sale siempre con 0, imprime la línea del módulo Pool,
//! guarda el payload por sesión y no deja a Claude esperando por una statusline encadenada colgada.

use std::io::Write;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("pool-it-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn run(dir: Option<&PathBuf>, payload: &str) -> (std::process::Output, Duration) {
    let started = Instant::now();
    let mut command = Command::new(env!("CARGO_BIN_EXE_poxiterminal-hook"));
    command.arg("claude-statusline");
    if let Some(dir) = dir {
        command.arg("--dir").arg(dir);
    }
    let mut child =
        command.stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::null()).spawn().unwrap();
    child.stdin.take().unwrap().write_all(payload.as_bytes()).unwrap();
    (child.wait_with_output().unwrap(), started.elapsed())
}

#[test]
fn prints_the_pool_line_and_stores_the_payload_per_session() {
    let dir = scratch("line");
    std::fs::write(dir.join("line.txt"), "Pool 18/50\n").unwrap();
    let payload = r#"{"session_id":"s-42","rate_limits":{"five_hour":{"used_percentage":21}}}"#;
    let (output, _) = run(Some(&dir), payload);
    assert!(output.status.success());
    assert_eq!(String::from_utf8_lossy(&output.stdout), "Pool 18/50\n");
    assert_eq!(std::fs::read_to_string(dir.join("statusline").join("s-42.json")).unwrap(), payload);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn without_a_dir_it_is_a_silent_no_op() {
    let (output, _) = run(None, r#"{"session_id":"x"}"#);
    assert!(output.status.success());
    assert!(output.stdout.is_empty());
}

#[test]
fn a_hung_chained_statusline_cannot_hold_claude_for_long() {
    let dir = scratch("hang");
    std::fs::write(dir.join("line.txt"), "Pool 1/2\n").unwrap();
    std::fs::write(dir.join("chain.txt"), "sleep 30").unwrap();
    let (output, elapsed) = run(Some(&dir), r#"{"session_id":"s-1"}"#);
    assert!(output.status.success());
    assert!(elapsed < Duration::from_secs(8), "tardó {elapsed:?}");
    // Nuestra línea sale aunque la encadenada no responda.
    assert_eq!(String::from_utf8_lossy(&output.stdout), "Pool 1/2\n");
    let _ = std::fs::remove_dir_all(&dir);
}
