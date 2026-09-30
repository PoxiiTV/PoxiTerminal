use std::net::Ipv6Addr;

use serde::{Deserialize, Serialize};

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SshHostProxyMode {
    #[default]
    Inherit,
    Direct,
    Socks5,
    Http,
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SshHostJumpMode {
    #[default]
    Inherit,
    None,
    Host,
}

#[derive(Debug, Default, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct SshConnectionOptions {
    pub proxy_mode: SshHostProxyMode,
    pub proxy_host: String,
    pub proxy_port: Option<u16>,
    pub proxy_username: String,
    pub jump_mode: SshHostJumpMode,
    pub jump_host: String,
    /// Túneles que se abren con cada pane conectado a este host.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub forwards: Vec<PortForward>,
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PortForwardKind {
    /// `-L`: 127.0.0.1:local → remote_host:remote_port vía SSH.
    #[default]
    Local,
    /// `-D`: proxy SOCKS5 en 127.0.0.1:local.
    Dynamic,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PortForward {
    #[serde(default)]
    pub kind: PortForwardKind,
    pub local_port: u16,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub remote_host: String,
    #[serde(default)]
    pub remote_port: u16,
    #[serde(default = "enabled_by_default")]
    pub enabled: bool,
}

fn enabled_by_default() -> bool {
    true
}

impl PortForward {
    /// "127.0.0.1:5432 → db:5432" / "SOCKS 127.0.0.1:1080".
    pub fn label(&self) -> String {
        match self.kind {
            PortForwardKind::Local => {
                format!("127.0.0.1:{} → {}:{}", self.local_port, self.remote_host, self.remote_port)
            },
            PortForwardKind::Dynamic => format!("SOCKS 127.0.0.1:{}", self.local_port),
        }
    }
}

/// Fallo de validación de un túnel; la UI lo traduce.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PortForwardError {
    /// Índice de fila con un puerto fuera de 1–65535.
    Port(usize),
    /// Índice de fila Local sin host remoto válido.
    RemoteHost(usize),
    /// Puerto local repetido.
    Duplicate(u16),
}

pub fn validate_forwards(forwards: &[PortForward]) -> Result<(), PortForwardError> {
    let mut seen = std::collections::HashSet::new();
    for (index, forward) in forwards.iter().enumerate() {
        let local = forward.kind == PortForwardKind::Local;
        if forward.local_port == 0 || (local && forward.remote_port == 0) {
            return Err(PortForwardError::Port(index));
        }
        if local && validate_host(forward.remote_host.trim_matches(['[', ']'])).is_err() {
            return Err(PortForwardError::RemoteHost(index));
        }
        if !seen.insert(forward.local_port) {
            return Err(PortForwardError::Duplicate(forward.local_port));
        }
    }
    Ok(())
}

impl SshConnectionOptions {
    pub fn is_default(&self) -> bool {
        self == &Self::default()
    }

    pub fn effective_proxy_port(&self) -> u16 {
        self.proxy_port.unwrap_or(match self.proxy_mode {
            SshHostProxyMode::Http => 8080,
            _ => 1080,
        })
    }

    pub fn has_custom_proxy(&self) -> bool {
        matches!(self.proxy_mode, SshHostProxyMode::Socks5 | SshHostProxyMode::Http)
    }

    pub fn normalized_proxy_host(&self) -> String {
        let host = self.proxy_host.trim();
        host.strip_prefix('[')
            .and_then(|host| host.strip_suffix(']'))
            .unwrap_or(host)
            .to_ascii_lowercase()
    }

    pub fn validate(&self, destination: &str) -> Result<(), String> {
        validate_ssh_destination(destination)?;
        if self.has_custom_proxy() {
            validate_host(&self.normalized_proxy_host()).map_err(|_| {
                "Dirección de proxy no válida: indica solo el nombre de host o la IP, sin protocolo, puerto ni contraseña".to_owned()
            })?;
            if self.effective_proxy_port() == 0 {
                return Err("El puerto del proxy debe estar entre 1 y 65535".to_owned());
            }
            let username = self.proxy_username.trim();
            if username.chars().any(char::is_control) {
                return Err("El usuario del proxy no puede contener caracteres de control".to_owned());
            }
            if self.proxy_mode == SshHostProxyMode::Socks5 && username.len() > 255 {
                return Err("El usuario SOCKS5 no puede superar los 255 bytes".to_owned());
            }
            if self.proxy_mode == SshHostProxyMode::Http && username.contains(':') {
                return Err("El usuario del proxy HTTP no puede contener dos puntos".to_owned());
            }
        }
        if self.jump_mode == SshHostJumpMode::Host {
            validate_ssh_destination(&self.jump_host)
                .map_err(|_| "Dirección de salto no válida: indica un único alias SSH o user@host:port".to_owned())?;
            if normalized_destination(&self.jump_host) == normalized_destination(destination) {
                return Err("El host de destino no puede ser su propio salto".to_owned());
            }
        }
        Ok(())
    }

    pub fn proxy_credential_target(&self, destination: &str) -> Option<String> {
        if !self.has_custom_proxy() || self.proxy_username.trim().is_empty() {
            return None;
        }
        use sha2::{Digest, Sha256};
        use std::fmt::Write as _;

        let mode = match self.proxy_mode {
            SshHostProxyMode::Socks5 => "socks5",
            SshHostProxyMode::Http => "http",
            _ => return None,
        };
        let mut digest = Sha256::new();
        for field in [
            destination.trim().to_owned(),
            mode.to_owned(),
            self.normalized_proxy_host(),
            self.effective_proxy_port().to_string(),
            self.proxy_username.trim().to_owned(),
        ] {
            digest.update((field.len() as u64).to_be_bytes());
            digest.update(field.as_bytes());
        }
        let mut fingerprint = String::with_capacity(64);
        for byte in digest.finalize() {
            let _ = write!(fingerprint, "{byte:02x}");
        }
        Some(format!("PoxiTerminal/SSH/Proxy/{fingerprint}"))
    }
}

pub(crate) fn validate_ssh_destination(value: &str) -> Result<(), String> {
    let value = value.trim();
    if value.is_empty()
        || value.chars().any(|character| {
            character.is_whitespace()
                || character.is_control()
                || ",;&|<>\"'`\\?#".contains(character)
        })
    {
        return Err("La dirección SSH está vacía o contiene caracteres no permitidos".to_owned());
    }
    let address = value.strip_prefix("ssh://").unwrap_or(value);
    if address.starts_with('-') {
        return Err("La dirección SSH no puede empezar como una opción (-)".to_owned());
    }
    let host_port = if let Some((username, host)) = address.rsplit_once('@') {
        if username.is_empty() || username.contains(['@', ':', '/', '[', ']']) {
            return Err("Usuario SSH no válido".to_owned());
        }
        host
    } else {
        address
    };
    let (host, port) = if let Some(rest) = host_port.strip_prefix('[') {
        let (host, suffix) = rest.split_once(']').ok_or("A la dirección IPv6 le falta el corchete de cierre")?;
        if host.parse::<Ipv6Addr>().is_err() {
            return Err("Dirección IPv6 no válida".to_owned());
        }
        let port = if suffix.is_empty() {
            None
        } else {
            Some(suffix.strip_prefix(':').ok_or("Formato de puerto SSH no válido")?)
        };
        (host, port)
    } else if let Some((host, port)) = host_port.rsplit_once(':') {
        if host.contains(':') { (host_port, None) } else { (host, Some(port)) }
    } else {
        (host_port, None)
    };
    validate_host(host)?;
    if port.is_some_and(|port| port.parse::<u16>().map_or(true, |port| port == 0)) {
        return Err("El puerto SSH debe estar entre 1 y 65535".to_owned());
    }
    Ok(())
}

fn validate_host(host: &str) -> Result<(), String> {
    if host.is_empty() || host.starts_with('-') || host.len() > 253 {
        return Err("Nombre de host no válido".to_owned());
    }
    if host.contains(':') {
        host.parse::<Ipv6Addr>().map_err(|_| "Dirección IPv6 no válida".to_owned())?;
    } else if host
        .chars()
        .any(|character| !character.is_alphanumeric() && !matches!(character, '.' | '-' | '_'))
    {
        return Err("El nombre de host contiene caracteres no permitidos".to_owned());
    }
    Ok(())
}

fn normalized_destination(destination: &str) -> String {
    let destination = destination.trim().strip_prefix("ssh://").unwrap_or(destination.trim());
    let (username, host) = destination.rsplit_once('@').unwrap_or(("", destination));
    let host = host.strip_suffix(":22").unwrap_or(host).to_ascii_lowercase();
    format!("{username}@{host}")
}

#[cfg(test)]
#[path = "connection/tests.rs"]
mod tests;
