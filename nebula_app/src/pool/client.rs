//! Cliente HTTP bloqueante hacia el servidor Fifty (`ureq`, ya presente en la app).

use std::fmt;
use std::time::Duration;

use serde_json::{Value, json};
use zeroize::Zeroizing;

use super::samples::Sample;
use super::state::ServerState;
use super::transcript::UsageEvent;

#[derive(Debug)]
pub enum Error {
    /// 401: token inválido o dispositivo revocado.
    Unauthorized,
    /// El servidor contestó con otro estado de error (p. ej. 400 por un lote inválido).
    Rejected(u16),
    /// Sin conexión, DNS, TLS, tiempo agotado...
    Network(String),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unauthorized => f.write_str("token rechazado (¿dispositivo revocado?)"),
            Self::Rejected(status) => write!(f, "el servidor respondió {status}"),
            Self::Network(why) => write!(f, "sin conexión: {why}"),
        }
    }
}

impl From<ureq::Error> for Error {
    fn from(error: ureq::Error) -> Self {
        match error {
            ureq::Error::StatusCode(401) => Self::Unauthorized,
            ureq::Error::StatusCode(status) => Self::Rejected(status),
            other => Self::Network(other.to_string()),
        }
    }
}

pub struct Client {
    agent: ureq::Agent,
    base: String,
    auth: Zeroizing<String>,
}

impl Client {
    pub fn new(base: &str, token: &str) -> Self {
        let config =
            ureq::config::Config::builder().timeout_global(Some(Duration::from_secs(20))).build();
        Self {
            agent: config.new_agent(),
            base: base.trim_end_matches('/').to_owned(),
            auth: Zeroizing::new(format!("Bearer {token}")),
        }
    }

    fn get(&self, path: &str) -> Result<Value, Error> {
        let mut response = self
            .agent
            .get(&format!("{}{path}", self.base))
            .header("Authorization", self.auth.as_str())
            .call()?;
        response.body_mut().read_json().map_err(|error| Error::Network(error.to_string()))
    }

    /// Comprueba el token. Devuelve (usuario, dispositivo).
    pub fn me(&self) -> Result<(String, String), Error> {
        let value = self.get("/api/v1/me")?;
        let text = |key: &str| value[key].as_str().unwrap_or_default().to_owned();
        Ok((text("user"), text("device")))
    }

    pub fn state(&self) -> Result<ServerState, Error> {
        let value = self.get("/api/v1/state")?;
        serde_json::from_value(value).map_err(|error| Error::Network(error.to_string()))
    }

    /// Envía un lote (máx. 200 muestras y 500 eventos). Reenviarlo es seguro: el servidor es
    /// idempotente.
    pub fn ingest(
        &self,
        sent_at: i64,
        samples: &[Sample],
        events: &[UsageEvent],
    ) -> Result<(), Error> {
        let body = json!({ "sent_at": sent_at, "samples": samples, "events": events });
        self.agent
            .post(&format!("{}/api/v1/ingest", self.base))
            .header("Authorization", self.auth.as_str())
            .send_json(&body)?;
        Ok(())
    }
}

/// Valida y normaliza la dirección del servidor. El token viaja en cada petición, así que se exige
/// HTTPS salvo en la propia máquina.
pub fn normalize_server(raw: &str) -> Result<String, String> {
    let url = raw.trim().trim_end_matches('/');
    let (scheme, rest) = url.split_once("://").ok_or("La dirección debe empezar por https://")?;
    let authority = rest.split('/').next().unwrap_or_default();
    if authority.is_empty() || authority.contains('@') {
        return Err("Dirección no válida (sin usuario ni contraseña en la URL)".into());
    }
    let host = match authority.strip_prefix('[') {
        Some(ipv6) => ipv6.split(']').next().unwrap_or_default(),
        None => authority.split(':').next().unwrap_or_default(),
    };
    let loopback = matches!(host, "localhost" | "127.0.0.1" | "::1");
    match scheme {
        "https" => Ok(url.to_owned()),
        "http" if loopback => Ok(url.to_owned()),
        _ => Err("Usa https:// (http solo se admite en localhost)".into()),
    }
}

#[cfg(test)]
mod tests {
    use std::io::{Read, Write};
    use std::net::TcpListener;

    use super::*;

    /// Servidor mínimo que contesta una vez y devuelve la petición recibida.
    fn one_shot(
        status_line: &'static str,
        body: &'static str,
    ) -> (String, std::thread::JoinHandle<String>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let handle = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut request = Vec::new();
            let mut buffer = [0u8; 4096];
            loop {
                let read = stream.read(&mut buffer).unwrap();
                request.extend_from_slice(&buffer[..read]);
                let text = String::from_utf8_lossy(&request);
                if let Some(head_end) = text.find("\r\n\r\n") {
                    let length = text
                        .lines()
                        .find_map(|l| {
                            l.to_ascii_lowercase()
                                .strip_prefix("content-length:")
                                .map(|v| v.trim().parse::<usize>().unwrap())
                        })
                        .unwrap_or(0);
                    if request.len() >= head_end + 4 + length {
                        break;
                    }
                }
            }
            let response = format!(
                "HTTP/1.1 {status_line}\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
                body.len()
            );
            stream.write_all(response.as_bytes()).unwrap();
            String::from_utf8_lossy(&request).into_owned()
        });
        (url, handle)
    }

    #[test]
    fn ingest_posts_the_whitelisted_shape_with_the_bearer_token() {
        let (url, server) = one_shot("200 OK", r#"{"samples":1,"events":0}"#);
        let sample = Sample {
            ts: 1_800_000_000_000,
            five_hour: Some(super::super::samples::Window {
                used_percentage: 21,
                resets_at: 1_800_003_600,
            }),
            seven_day: None,
            session_id: Some("s1".into()),
        };
        Client::new(&url, "cmk_secreto").ingest(1_800_000_000_500, &[sample], &[]).unwrap();
        let request = server.join().unwrap();
        assert!(request.starts_with("POST /api/v1/ingest "), "{request}");
        assert!(request.to_ascii_lowercase().contains("authorization: bearer cmk_secreto"));
        let body: Value = serde_json::from_str(request.split("\r\n\r\n").nth(1).unwrap()).unwrap();
        assert_eq!(body["sent_at"], 1_800_000_000_500_i64);
        assert_eq!(body["samples"][0]["five_hour"]["used_percentage"], 21);
        assert!(body["samples"][0].get("seven_day").is_none());
        assert_eq!(body["events"], json!([]));
    }

    #[test]
    fn me_and_state_are_parsed_and_errors_are_classified() {
        let (url, _server) = one_shot("200 OK", r#"{"user":"alexis","device":"pc"}"#);
        assert_eq!(Client::new(&url, "t").me().unwrap(), ("alexis".into(), "pc".into()));

        let (url, _server) = one_shot("401 Unauthorized", r#"{"error":"unauthorized"}"#);
        assert!(matches!(Client::new(&url, "t").me(), Err(Error::Unauthorized)));

        let (url, _server) = one_shot("400 Bad Request", r#"{"error":"invalid_payload"}"#);
        assert!(matches!(Client::new(&url, "t").ingest(1, &[], &[]), Err(Error::Rejected(400))));

        // puerto cerrado
        let closed = TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}", closed.local_addr().unwrap());
        drop(closed);
        assert!(matches!(Client::new(&url, "t").state(), Err(Error::Network(_))));
    }

    #[test]
    fn server_address_must_be_https_except_on_this_machine() {
        assert_eq!(
            normalize_server(" https://pool.ejemplo.com/ ").unwrap(),
            "https://pool.ejemplo.com"
        );
        assert!(normalize_server("http://localhost:4532").is_ok());
        assert!(normalize_server("http://127.0.0.1:4532/").is_ok());
        assert!(normalize_server("http://[::1]:4532").is_ok());
        assert!(normalize_server("http://192.168.1.82:4532").is_err());
        assert!(normalize_server("http://pool.ejemplo.com").is_err());
        assert!(normalize_server("pool.ejemplo.com").is_err());
        assert!(normalize_server("https://user:pass@pool.ejemplo.com").is_err());
        assert!(normalize_server("https://").is_err());
        assert!(normalize_server("ftp://localhost").is_err());
    }
}
