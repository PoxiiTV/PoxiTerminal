//! Configuración del módulo Pool: ajustes en `pebrel_settings.txt` y token en el almacén seguro.

use std::path::PathBuf;

use zeroize::Zeroizing;

/// Entrada del almacén de credenciales del sistema (Credential Manager en Windows).
const TOKEN_TARGET: &str = "PoxiTerminal/Pool/token";

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Config {
    pub enabled: bool,
    pub server: Option<String>,
}

pub fn load() -> Config {
    let raw = nebula_settings::RawSettings::load();
    Config {
        enabled: raw.bool_on("pool_enabled").unwrap_or(false),
        server: raw.value("pool_server").map(str::to_owned).filter(|url| !url.is_empty()),
    }
}

pub fn save(enabled: bool, server: Option<&str>) -> std::io::Result<()> {
    let mut updates = vec![("pool_enabled", if enabled { "on" } else { "off" }.to_owned())];
    if let Some(server) = server {
        updates.push(("pool_server", server.to_owned()));
    }
    nebula_settings::persist_keys(&updates)
}

pub fn token() -> Option<Zeroizing<String>> {
    let bytes = crate::ssh_credentials::load_generic_secret(TOKEN_TARGET).ok().flatten()?;
    String::from_utf8(bytes).ok().map(|text| Zeroizing::new(text.trim().to_owned()))
}

pub fn store_token(token: &str) -> std::io::Result<()> {
    crate::ssh_credentials::store_generic_secret(TOKEN_TARGET, token.as_bytes())
}

pub fn forget_token() -> std::io::Result<()> {
    crate::ssh_credentials::delete_generic_secret(TOKEN_TARGET)
}

/// Carpeta de trabajo del módulo: payloads de la statusline, offsets, línea de estado, cadena.
pub fn pool_dir() -> PathBuf {
    crate::platform::dirs::data_dir().join("pool")
}
