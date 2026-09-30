use super::*;
use russh::keys::ssh_key::{Algorithm, PrivateKey};
use russh::{client, server};
use std::sync::Arc;
use tokio::sync::mpsc;

fn check(future: impl std::future::Future<Output = ()>) {
    tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap().block_on(async {
        tokio::time::timeout(Duration::from_secs(10), future).await.expect("tunnel test timed out");
    });
}

/// Ejecuta el parser SOCKS contra un cliente en memoria.
async fn parse(client_bytes: &[u8]) -> (io::Result<(String, u16)>, Vec<u8>) {
    let (mut client, mut server) = tokio::io::duplex(1024);
    client.write_all(client_bytes).await.unwrap();
    let result = socks_request(&mut server).await;
    drop(server);
    let mut replies = Vec::new();
    client.read_to_end(&mut replies).await.unwrap();
    (result, replies)
}

const GREETING: [u8; 3] = [5, 1, 0];

#[test]
fn socks_parses_ipv4_domain_and_ipv6_connect() {
    check(async {
        let ipv4 = [&GREETING[..], &[5, 1, 0, 1, 10, 0, 0, 7, 0x1F, 0x90]].concat();
        let (result, replies) = parse(&ipv4).await;
        assert_eq!(result.unwrap(), ("10.0.0.7".to_owned(), 8080));
        assert_eq!(replies, [5, 0]);

        let domain = [&GREETING[..], &[5, 1, 0, 3, 7], b"db.corp", &[0x15, 0x38]].concat();
        assert_eq!(parse(&domain).await.0.unwrap(), ("db.corp".to_owned(), 5432));

        let mut ipv6 = [&GREETING[..], &[5, 1, 0, 4]].concat();
        ipv6.extend_from_slice(&Ipv6Addr::LOCALHOST.octets());
        ipv6.extend_from_slice(&443u16.to_be_bytes());
        assert_eq!(parse(&ipv6).await.0.unwrap(), ("::1".to_owned(), 443));
    });
}

#[test]
fn socks_rejects_unsupported_commands_addresses_and_auth() {
    check(async {
        // BIND → "command not supported".
        let bind = [&GREETING[..], &[5, 2, 0, 1, 127, 0, 0, 1, 0, 80]].concat();
        let (result, replies) = parse(&bind).await;
        assert!(result.is_err());
        assert_eq!(replies, [5, 0, 5, REPLY_COMMAND_UNSUPPORTED, 0, 1, 0, 0, 0, 0, 0, 0]);

        let address = [&GREETING[..], &[5, 1, 0, 9]].concat();
        let (result, replies) = parse(&address).await;
        assert!(result.is_err());
        assert_eq!(replies[3], REPLY_ADDRESS_UNSUPPORTED);

        // Solo usuario/contraseña ofrecido → sin método aceptable.
        let (result, replies) = parse(&[5, 1, 2]).await;
        assert!(result.is_err());
        assert_eq!(replies, [5, 0xFF]);

        assert!(parse(&[4, 1, 0]).await.0.is_err());
    });
}

// ---- Túneles reales contra un servidor russh en memoria ----

struct Echo {
    opened: mpsc::UnboundedSender<(String, u32)>,
    streams: JoinSet<()>,
}

impl server::Handler for Echo {
    type Error = russh::Error;

    async fn auth_none(&mut self, _: &str) -> Result<server::Auth, Self::Error> {
        Ok(server::Auth::Accept)
    }

    async fn channel_open_direct_tcpip(
        &mut self,
        channel: russh::Channel<server::Msg>,
        host: &str,
        port: u32,
        _: &str,
        _: u32,
        reply: server::ChannelOpenHandle,
        _: &mut server::Session,
    ) -> Result<(), Self::Error> {
        if port == 1 {
            reply.reject(russh::ChannelOpenFailure::ConnectFailed).await;
            return Ok(());
        }
        reply.accept().await;
        self.opened.send((host.to_owned(), port)).unwrap();
        self.streams.spawn(async move {
            let mut stream = channel.into_stream();
            let mut bytes = [0; 128];
            while let Ok(count) = stream.read(&mut bytes).await {
                if count == 0 || stream.write_all(&bytes[..count]).await.is_err() {
                    break;
                }
            }
            let _ = stream.shutdown().await;
        });
        Ok(())
    }
}

struct Fixture {
    session: SharedSession,
    opened: mpsc::UnboundedReceiver<(String, u32)>,
    server: JoinHandle<()>,
    _directory: tempfile::TempDir,
}

impl Drop for Fixture {
    fn drop(&mut self) {
        self.server.abort();
    }
}

impl Fixture {
    async fn new() -> Self {
        let directory = tempfile::tempdir().unwrap();
        let known_hosts = directory.path().join("known_hosts");
        let key = PrivateKey::random(&mut rand::rng(), Algorithm::Ed25519).unwrap();
        russh::keys::known_hosts::learn_known_hosts_path(
            "tunnel.test",
            22,
            key.public_key(),
            &known_hosts,
        )
        .unwrap();
        let config = Arc::new(server::Config {
            keys: vec![key],
            auth_rejection_time: Duration::ZERO,
            auth_rejection_time_initial: Some(Duration::ZERO),
            ..Default::default()
        });
        let (client_io, server_io) = tokio::io::duplex(65536);
        let (sender, opened) = mpsc::unbounded_channel();
        let server = tokio::spawn(async move {
            let handler = Echo { opened: sender, streams: JoinSet::new() };
            let session = server::run_stream(config, server_io, handler).await.unwrap();
            let _ = session.await;
        });
        let mut session = client::connect_stream(
            Arc::new(client::Config::default()),
            client_io,
            super::super::ClientHandler {
                host: "tunnel.test".into(),
                port: 22,
                allow_prompt: false,
                handshake: super::super::lifecycle::Handshake::default(),
                known_hosts_path: Some(known_hosts),
            },
        )
        .await
        .unwrap();
        assert!(session.authenticate_none("fixture").await.unwrap().success());
        Self { session: Arc::new(session), opened, server, _directory: directory }
    }
}

fn free_port() -> u16 {
    std::net::TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap().local_addr().unwrap().port()
}

fn forward(kind: PortForwardKind, local_port: u16, remote_port: u16) -> PortForward {
    PortForward { kind, local_port, remote_host: "db".into(), remote_port, enabled: true }
}

async fn echo(socket: &mut TcpStream, payload: &[u8]) {
    socket.write_all(payload).await.unwrap();
    let mut back = vec![0; payload.len()];
    socket.read_exact(&mut back).await.unwrap();
    assert_eq!(back, payload);
}

#[test]
fn local_and_socks_tunnels_forward_and_release_on_drop() {
    check(async {
        let mut fixture = Fixture::new().await;
        let (local_port, socks_port, busy) = (free_port(), free_port(), free_port());
        let _occupied = TcpListener::bind((Ipv4Addr::LOCALHOST, busy)).await.unwrap();
        let mut disabled = forward(PortForwardKind::Local, free_port(), 80);
        disabled.enabled = false;
        let (tunnels, report) = open_tunnels(
            &fixture.session,
            &[
                forward(PortForwardKind::Local, local_port, 5432),
                forward(PortForwardKind::Dynamic, socks_port, 0),
                forward(PortForwardKind::Local, busy, 80),
                disabled,
            ],
        )
        .await;
        assert_eq!(tunnels.len(), 2);
        assert_eq!(
            report.active,
            [format!("127.0.0.1:{local_port} → db:5432"), format!("SOCKS 127.0.0.1:{socks_port}")]
        );
        assert_eq!(report.failed.len(), 1);
        assert_eq!(report.failed[0].0, busy);

        let mut direct = TcpStream::connect((Ipv4Addr::LOCALHOST, local_port)).await.unwrap();
        echo(&mut direct, b"local payload").await;
        assert_eq!(fixture.opened.recv().await.unwrap(), ("db".to_owned(), 5432));

        let mut socks = TcpStream::connect((Ipv4Addr::LOCALHOST, socks_port)).await.unwrap();
        socks.write_all(&[5, 1, 0, 5, 1, 0, 3, 4, b'h', b'o', b's', b't', 0, 22]).await.unwrap();
        let mut replies = [0; 12];
        socks.read_exact(&mut replies).await.unwrap();
        assert_eq!(replies[..4], [5, 0, 5, REPLY_OK]);
        echo(&mut socks, b"socks payload").await;
        assert_eq!(fixture.opened.recv().await.unwrap(), ("host".to_owned(), 22));

        drop(tunnels);
        let mut byte = [0];
        assert!(matches!(direct.read(&mut byte).await, Ok(0) | Err(_)));
        assert!(matches!(socks.read(&mut byte).await, Ok(0) | Err(_)));
        TcpListener::bind((Ipv4Addr::LOCALHOST, local_port)).await.expect("listener released");
        TcpListener::bind((Ipv4Addr::LOCALHOST, socks_port)).await.expect("listener released");
        assert!(!fixture.session.is_closed(), "the shared SSH session survives");
    });
}

#[test]
fn socks_reports_rejected_channels_and_invalid_rules_open_nothing() {
    check(async {
        let fixture = Fixture::new().await;
        let port = free_port();
        let (tunnels, _) =
            open_tunnels(&fixture.session, &[forward(PortForwardKind::Dynamic, port, 0)]).await;
        let mut socks = TcpStream::connect((Ipv4Addr::LOCALHOST, port)).await.unwrap();
        socks.write_all(&[5, 1, 0, 5, 1, 0, 1, 127, 0, 0, 1, 0, 1]).await.unwrap();
        let mut replies = [0; 12];
        socks.read_exact(&mut replies).await.unwrap();
        assert_eq!(replies[3], REPLY_FAILURE);
        drop(tunnels);

        let (tunnels, report) =
            open_tunnels(&fixture.session, &[forward(PortForwardKind::Local, 0, 80)]).await;
        assert!(tunnels.is_empty() && report == TunnelReport::default());
    });
}
