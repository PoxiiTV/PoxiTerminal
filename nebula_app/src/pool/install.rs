//! Instalación de la statusline de Claude Code (`statusLine` en `~/.claude/settings.json`).
//!
//! Claude solo admite un comando de statusline. Si el usuario ya tiene uno (p. ej. Orca), se guarda
//! en `<pool>/chain.txt` y nuestro helper lo ejecuta con el mismo payload; al desactivar el módulo
//! se restaura exactamente el original. Nunca se toca nada más del archivo.

use std::path::{Path, PathBuf};

use serde_json::{Value, json};

const CHAIN_FILE: &str = "chain.txt";
const ORIGINAL_FILE: &str = "statusline-original.json";

/// ¿Es un comando de statusline instalado por nosotros?
pub fn is_ours(command: &str) -> bool {
    command.contains("poxiterminal-hook") && command.contains("claude-statusline")
}

fn quote(arg: &str) -> String {
    if arg.chars().all(|c| c.is_ascii_alphanumeric() || "/._-:~".contains(c)) {
        arg.to_owned()
    } else {
        format!("\"{arg}\"")
    }
}

/// `<helper> claude-statusline --dir <pool>`; barras normales para que sirva en cualquier shell.
pub fn our_command(helper: &str, pool_dir: &Path) -> String {
    let dir = pool_dir.to_string_lossy().replace('\\', "/");
    format!("{} claude-statusline --dir {}", quote(helper), quote(&dir))
}

#[derive(Debug, PartialEq)]
pub struct Outcome {
    pub changed: bool,
    /// La statusline anterior de otra herramienta, si la había y la hemos desplazado.
    pub displaced: Option<Value>,
}

/// Cirugía JSON pura. `None` si la raíz no es un objeto (formato inesperado: no se toca).
pub fn enable_into(root: &mut Value, ours: &str) -> Option<Outcome> {
    let object = root.as_object_mut()?;
    let current = object.get("statusLine").cloned();
    if let Some(existing) =
        current.as_ref().and_then(|c| c["command"].as_str()).filter(|c| is_ours(c))
    {
        if existing == ours {
            return Some(Outcome { changed: false, displaced: None });
        }
        // Es nuestra pero apunta a otra ruta (la app se movió): se actualiza en sitio.
        object.get_mut("statusLine")?["command"] = json!(ours);
        return Some(Outcome { changed: true, displaced: None });
    }
    let mut entry = json!({ "type": "command", "command": ours });
    if let Some(padding) = current.as_ref().and_then(|c| c.get("padding")) {
        entry["padding"] = padding.clone();
    }
    object.insert("statusLine".into(), entry);
    Some(Outcome { changed: true, displaced: current })
}

/// Quita nuestra statusline y restaura `original` (o borra la clave si no había). `false` si la que
/// hay no es nuestra: no se toca.
pub fn disable_from(root: &mut Value, original: Option<Value>) -> bool {
    let Some(object) = root.as_object_mut() else { return false };
    let ours =
        object.get("statusLine").and_then(|status| status["command"].as_str()).is_some_and(is_ours);
    if !ours {
        return false;
    }
    match original {
        Some(previous) => {
            object.insert("statusLine".into(), previous);
        },
        None => {
            object.shift_remove("statusLine");
        },
    }
    true
}

/// Directorio de configuración de Claude Code (respeta `CLAUDE_CONFIG_DIR`).
pub fn claude_dir() -> Option<PathBuf> {
    crate::ai_hook::claude_config_dir()
}

/// Aplica `enable` sobre `settings` usando `pool_dir` para guardar la cadena. `Ok(true)` si cambió el
/// archivo. `helper` es el comando del helper (solo hace falta al activar).
pub fn apply_at(
    settings: &Path,
    pool_dir: &Path,
    enable: bool,
    helper: Option<&str>,
) -> Result<bool, String> {
    let Ok(Some(_lock)) = crate::atomic_file::try_lock(settings) else {
        return Err("settings.json está en uso; se reintentará".into());
    };
    let mut root: Value = match std::fs::read_to_string(settings) {
        Ok(raw) => serde_json::from_str(&raw).map_err(|error| {
            format!("{} no es JSON válido ({error}); no se toca", settings.display())
        })?,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound && enable => json!({}),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(false),
        Err(error) => return Err(format!("no se puede leer {}: {error}", settings.display())),
    };

    let changed = if enable {
        let helper = helper.ok_or("no se encuentra poxiterminal-hook junto a la app")?;
        let outcome = enable_into(&mut root, &our_command(helper, pool_dir))
            .ok_or("settings.json tiene un formato inesperado; no se toca")?;
        if let Some(previous) = &outcome.displaced {
            save_displaced(pool_dir, previous)?;
        }
        outcome.changed
    } else {
        let original = std::fs::read_to_string(pool_dir.join(ORIGINAL_FILE))
            .ok()
            .and_then(|raw| serde_json::from_str(&raw).ok());
        let changed = disable_from(&mut root, original);
        let _ = std::fs::remove_file(pool_dir.join(CHAIN_FILE));
        let _ = std::fs::remove_file(pool_dir.join(ORIGINAL_FILE));
        let _ = std::fs::remove_file(pool_dir.join("line.txt"));
        changed
    };

    if changed {
        backup_once(settings)?;
        let raw = serde_json::to_string_pretty(&root).map_err(|error| error.to_string())?;
        crate::atomic_file::write(settings, raw.as_bytes())
            .map_err(|error| format!("no se pudo escribir {}: {error}", settings.display()))?;
    }
    Ok(changed)
}

fn save_displaced(pool_dir: &Path, previous: &Value) -> Result<(), String> {
    std::fs::create_dir_all(pool_dir).map_err(|error| error.to_string())?;
    let raw = serde_json::to_string_pretty(previous).map_err(|error| error.to_string())?;
    std::fs::write(pool_dir.join(ORIGINAL_FILE), raw).map_err(|error| error.to_string())?;
    match previous["command"].as_str().filter(|command| !is_ours(command)) {
        Some(command) => std::fs::write(pool_dir.join(CHAIN_FILE), command),
        None => match std::fs::remove_file(pool_dir.join(CHAIN_FILE)) {
            Err(error) if error.kind() != std::io::ErrorKind::NotFound => Err(error),
            _ => Ok(()),
        },
    }
    .map_err(|error| error.to_string())
}

/// Copia intacta de `settings.json` antes del primer cambio del módulo.
fn backup_once(settings: &Path) -> Result<(), String> {
    let backup = settings.with_extension("json.pool-bak");
    if settings.exists() && !backup.exists() {
        std::fs::copy(settings, &backup)
            .map_err(|error| format!("no se pudo crear la copia de seguridad: {error}"))?;
    }
    Ok(())
}

/// ¿Está nuestra statusline instalada ahora mismo?
pub fn is_installed(settings: &Path) -> bool {
    std::fs::read_to_string(settings)
        .ok()
        .and_then(|raw| serde_json::from_str::<Value>(&raw).ok())
        .and_then(|root| root["statusLine"]["command"].as_str().map(is_ours))
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    const HELPER: &str =
        "C:/Users/Alexis/AppData/Local/Programs/PoxiTerminal/runtime/poxiterminal-hook.exe";
    const ORCA: &str = r#"if [ -z "${HOME-}" ]; then echo "x"; fi"#;

    fn ours() -> String {
        our_command(HELPER, Path::new(r"C:\Users\Alexis\AppData\Roaming\PoxiTerminal\pool"))
    }

    #[test]
    fn command_uses_forward_slashes_and_quotes_only_when_needed() {
        assert_eq!(
            ours(),
            format!(
                "{HELPER} claude-statusline --dir C:/Users/Alexis/AppData/Roaming/PoxiTerminal/pool"
            )
        );
        let spaced =
            our_command("C:/Program Files/P/poxiterminal-hook.exe", Path::new("D:/mis datos/pool"));
        assert_eq!(
            spaced,
            "\"C:/Program Files/P/poxiterminal-hook.exe\" claude-statusline --dir \"D:/mis datos/pool\""
        );
        assert!(is_ours(&ours()) && is_ours(&spaced) && !is_ours(ORCA));
    }

    #[test]
    fn enabling_without_a_previous_statusline_just_adds_it_and_keeps_everything_else() {
        let mut root = json!({"model":"opus","hooks":{"Stop":[]},"env":{"A":"1"}});
        let outcome = enable_into(&mut root, &ours()).unwrap();
        assert_eq!(outcome, Outcome { changed: true, displaced: None });
        assert_eq!(root["statusLine"]["type"], "command");
        assert_eq!(root["model"], "opus");
        assert_eq!(root["hooks"], json!({"Stop":[]}));
        // idempotente
        assert!(!enable_into(&mut root, &ours()).unwrap().changed);
    }

    #[test]
    fn an_existing_statusline_is_displaced_with_its_padding_kept() {
        let previous = json!({"type":"command","command":ORCA,"padding":2});
        let mut root = json!({"statusLine": previous.clone(), "z": 1});
        let outcome = enable_into(&mut root, &ours()).unwrap();
        assert_eq!(outcome.displaced, Some(previous));
        assert_eq!(root["statusLine"]["padding"], 2);
        assert!(is_ours(root["statusLine"]["command"].as_str().unwrap()));
        assert_eq!(root["z"], 1);
    }

    #[test]
    fn a_stale_path_of_ours_is_updated_in_place_without_displacing_anything() {
        let mut root = json!({"statusLine":{"type":"command","command":"D:/viejo/poxiterminal-hook.exe claude-statusline --dir D:/viejo"}});
        let outcome = enable_into(&mut root, &ours()).unwrap();
        assert_eq!(outcome, Outcome { changed: true, displaced: None });
        assert_eq!(root["statusLine"]["command"], ours());
    }

    #[test]
    fn disabling_restores_the_original_or_removes_the_key_and_never_touches_foreign_ones() {
        let original = json!({"type":"command","command":ORCA});
        let mut root = json!({"a":1,"statusLine":{"type":"command","command":ours()},"b":2});
        assert!(disable_from(&mut root, Some(original.clone())));
        assert_eq!(root["statusLine"], original);

        let mut none = json!({"a":1,"statusLine":{"type":"command","command":ours()},"b":2});
        assert!(disable_from(&mut none, None));
        assert_eq!(none, json!({"a":1,"b":2}));

        let mut foreign = json!({"statusLine":{"type":"command","command":ORCA}});
        assert!(!disable_from(&mut foreign, None));
        assert_eq!(foreign["statusLine"]["command"], ORCA);
        assert!(enable_into(&mut json!([]), &ours()).is_none());
    }

    fn scratch(name: &str) -> (PathBuf, PathBuf) {
        let base = std::env::temp_dir().join(format!("pool-install-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        std::fs::create_dir_all(&base).unwrap();
        (base.join("settings.json"), base.join("pool"))
    }

    #[test]
    fn full_cycle_on_disk_chains_orca_and_restores_it_byte_for_byte_in_meaning() {
        let (settings, pool) = scratch("cycle");
        let original = json!({"model":"opus","statusLine":{"type":"command","command":ORCA}});
        std::fs::write(&settings, serde_json::to_string_pretty(&original).unwrap()).unwrap();

        assert!(apply_at(&settings, &pool, true, Some(HELPER)).unwrap());
        assert_eq!(std::fs::read_to_string(pool.join("chain.txt")).unwrap(), ORCA);
        assert!(is_installed(&settings));
        assert!(settings.with_extension("json.pool-bak").exists());
        assert!(
            !apply_at(&settings, &pool, true, Some(HELPER)).unwrap(),
            "segunda vez: sin cambios"
        );

        assert!(apply_at(&settings, &pool, false, None).unwrap());
        let restored: Value =
            serde_json::from_str(&std::fs::read_to_string(&settings).unwrap()).unwrap();
        assert_eq!(restored, original);
        assert!(
            !pool.join("chain.txt").exists() && !pool.join("statusline-original.json").exists()
        );
        assert!(!is_installed(&settings));
    }

    #[test]
    fn broken_json_is_left_alone_and_a_missing_file_is_created_only_when_enabling() {
        let (settings, pool) = scratch("broken");
        std::fs::write(&settings, "{ roto").unwrap();
        assert!(apply_at(&settings, &pool, true, Some(HELPER)).is_err());
        assert_eq!(std::fs::read_to_string(&settings).unwrap(), "{ roto");

        let (missing, pool) = scratch("missing");
        assert_eq!(apply_at(&missing, &pool, false, None), Ok(false));
        assert!(!missing.exists());
        assert!(apply_at(&missing, &pool, true, Some(HELPER)).unwrap());
        assert!(is_installed(&missing));
    }
}
