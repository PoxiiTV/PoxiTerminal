//! Túneles SSH de un pane: `-L` (local) y `-D` (SOCKS5). Viven dentro de la
//! tarea de sesión del pane; al soltarlos se aborta el listener y, con él, el
//! `JoinSet` de conexiones.

use std::io;
use std::net::{Ipv4Addr, Ipv6Addr, SocketAddr};
use std::time::Duration;

use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::task::{JoinHandle, JoinSet};

use super::{SessionError, SharedSession};
use crate::ssh_profiles::{PortForward, PortForwardKind};

// Límite de canales por listener; el resto espera en el backlog TCP.
const MAX_CONNECTIONS: usize = 64;
// Un cliente SOCKS que no completa la petición no puede retener un hueco.
const SOCKS_HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(10);

/// Estado de los túneles de un pane, para la UI.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TunnelReport {
    /// Etiquetas de los túneles escuchando.
    pub active: Vec<String>,
    /// Puerto local que no se pudo abrir + causa.
    pub failed: Vec<(u16, String)>,
}

pub(super) struct Tunnel(JoinHandle<()>);

impl Drop for Tunnel {
    fn drop(&mut self) {
        self.0.abort();
    }
}

/// Abre los túneles activos del host sobre la sesión ya autenticada del pane.
pub(super) async fn open_tunnels(
    session: &SharedSession,
    forwards: &[PortForward],
) -> (Vec<Tunnel>, TunnelReport) {
    let mut tunnels = Vec::new();
    let mut report = TunnelReport::default();
    let valid = crate::ssh_profiles::validate_forwards(forwards).is_ok();
    for forward in forwards.iter().filter(|forward| forward.enabled && valid) {
        match bind(session.clone(), forward.clone()).await {
            Ok(tunnel) => {
                report.active.push(forward.label());
                tunnels.push(tunnel);
            },
            Err(error) => {
                log::warn!("No se pudo abrir el túnel {}: {error}", forward.label());
                report.failed.push((forward.local_port, error.to_string()));
            },
        }
    }
    (tunnels, report)
}

async fn bind(session: SharedSession, forward: PortForward) -> io::Result<Tunnel> {
    // Solo loopback: el túnel nunca queda expuesto a la red local.
    let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, forward.local_port)).await?;
    Ok(Tunnel(tokio::spawn(serve(listener, session, forward))))
}

async fn serve(listener: TcpListener, session: SharedSession, forward: PortForward) {
    let mut connections = JoinSet::new();
    let mut accept_error_logged = false;
    loop {
        tokio::select! {
            accepted = listener.accept(), if connections.len() < MAX_CONNECTIONS => match accepted {
                Ok((local, peer)) => {
                    accept_error_logged = false;
                    connections.spawn(connection(local, peer, session.clone(), forward.clone()));
                },
                Err(error) => {
                    if !accept_error_logged {
                        log::warn!("El túnel SSH no pudo aceptar una conexión; reintentando: {error}");
                        accept_error_logged = true;
                    }
                    // ponytail: reintento fijo cada segundo; backoff si hay errores persistentes.
                    tokio::time::sleep(Duration::from_secs(1)).await;
                },
            },
            Some(result) = connections.join_next(), if !connections.is_empty() => {
                if let Ok(Err(error)) = result {
                    log::debug!("Conexión del túnel SSH cerrada: {error}");
                }
            },
        }
    }
}

async fn connection(
    mut local: TcpStream,
    peer: SocketAddr,
    session: SharedSession,
    forward: PortForward,
) -> Result<(), SessionError> {
    let socks = forward.kind == PortForwardKind::Dynamic;
    let (host, port) = if socks {
        tokio::time::timeout(SOCKS_HANDSHAKE_TIMEOUT, socks_request(&mut local))
            .await
            .map_err(|_| io::Error::new(io::ErrorKind::TimedOut, "SOCKS handshake"))??
    } else {
        (forward.remote_host.clone(), forward.remote_port)
    };
    let channel = super::lifecycle::network(
        "tunnel channel",
        session.channel_open_direct_tcpip(
            host,
            u32::from(port),
            peer.ip().to_string(),
            u32::from(peer.port()),
        ),
    )
    .await;
    if socks {
        socks_reply(&mut local, if channel.is_ok() { REPLY_OK } else { REPLY_FAILURE }).await?;
    }
    let mut remote = channel?.into_stream();
    tokio::io::copy_bidirectional(&mut local, &mut remote).await?;
    Ok(())
}

const SOCKS_VERSION: u8 = 5;
const REPLY_OK: u8 = 0x00;
const REPLY_FAILURE: u8 = 0x01;
const REPLY_COMMAND_UNSUPPORTED: u8 = 0x07;
const REPLY_ADDRESS_UNSUPPORTED: u8 = 0x08;

fn invalid(message: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, format!("SOCKS5: {message}"))
}

/// Saludo + petición SOCKS5 (RFC 1928): sin autenticación, solo CONNECT.
/// Devuelve el destino; los errores de protocolo ya se han respondido.
async fn socks_request<S: AsyncRead + AsyncWrite + Unpin>(
    stream: &mut S,
) -> io::Result<(String, u16)> {
    let mut header = [0; 2];
    stream.read_exact(&mut header).await?;
    if header[0] != SOCKS_VERSION {
        return Err(invalid("versión no soportada"));
    }
    let mut methods = vec![0; usize::from(header[1])];
    stream.read_exact(&mut methods).await?;
    if !methods.contains(&0x00) {
        stream.write_all(&[SOCKS_VERSION, 0xFF]).await?;
        return Err(invalid("el cliente exige autenticación"));
    }
    stream.write_all(&[SOCKS_VERSION, 0x00]).await?;

    let mut request = [0; 4];
    stream.read_exact(&mut request).await?;
    if request[0] != SOCKS_VERSION {
        return Err(invalid("versión no soportada"));
    }
    if request[1] != 0x01 {
        socks_reply(stream, REPLY_COMMAND_UNSUPPORTED).await?;
        return Err(invalid("solo se admite CONNECT"));
    }
    let host = match request[3] {
        0x01 => {
            let mut octets = [0; 4];
            stream.read_exact(&mut octets).await?;
            Ipv4Addr::from(octets).to_string()
        },
        0x03 => {
            let mut length = [0; 1];
            stream.read_exact(&mut length).await?;
            let mut name = vec![0; usize::from(length[0])];
            stream.read_exact(&mut name).await?;
            String::from_utf8(name).map_err(|_| invalid("dominio no válido"))?
        },
        0x04 => {
            let mut octets = [0; 16];
            stream.read_exact(&mut octets).await?;
            Ipv6Addr::from(octets).to_string()
        },
        _ => {
            socks_reply(stream, REPLY_ADDRESS_UNSUPPORTED).await?;
            return Err(invalid("tipo de dirección no soportado"));
        },
    };
    let mut port = [0; 2];
    stream.read_exact(&mut port).await?;
    Ok((host, u16::from_be_bytes(port)))
}

async fn socks_reply<S: AsyncWrite + Unpin>(stream: &mut S, code: u8) -> io::Result<()> {
    // BND.ADDR 0.0.0.0:0: los clientes no lo usan para CONNECT.
    stream.write_all(&[SOCKS_VERSION, code, 0x00, 0x01, 0, 0, 0, 0, 0, 0]).await
}

#[cfg(test)]
mod tests;
