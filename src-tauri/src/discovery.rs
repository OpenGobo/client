use opengobo_protocol::{
    ClientPacket, ServerHello, ServerPacket, ShowSnapshot, StateDelta,
};
use quinn::crypto::rustls::QuicClientConfig;
use quinn::{ClientConfig, Endpoint, TransportConfig};
use rustls_pki_types::{CertificateDer, ServerName, UnixTime};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::net::{IpAddr, Ipv4Addr, SocketAddr, ToSocketAddrs, UdpSocket};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::sync::{mpsc, oneshot};

const DEFAULT_DISCOVERY_PORT: u16 = 9513;
const MULTICAST_GROUP: Ipv4Addr = Ipv4Addr::new(239, 255, 71, 66);

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "type")]
enum DiscoveryWirePacket {
    DiscoverRequest,
    ServerAnnounce {
        server_id: String,
        server_port: u16,
        hello: ServerHello,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiscoveredServer {
    pub server_id: String,
    pub host: String,
    pub port: u16,
    pub show_title: String,
    pub protocol_version: u32,
    pub setup_mode_required: bool,
}

pub fn scan_lan(timeout_ms: u64) -> Result<Vec<DiscoveredServer>, String> {
    let socket = UdpSocket::bind("0.0.0.0:0").map_err(|e| e.to_string())?;
    let _ = socket.set_broadcast(true);
    let _ = socket.set_read_timeout(Some(Duration::from_millis(60)));

    let req =
        serde_json::to_vec(&DiscoveryWirePacket::DiscoverRequest).map_err(|e| e.to_string())?;
    let targets = [
        SocketAddr::from((Ipv4Addr::LOCALHOST, DEFAULT_DISCOVERY_PORT)),
        SocketAddr::from((MULTICAST_GROUP, DEFAULT_DISCOVERY_PORT)),
        SocketAddr::from((Ipv4Addr::BROADCAST, DEFAULT_DISCOVERY_PORT)),
    ];

    for target in targets {
        let _ = socket.send_to(&req, target);
    }

    let deadline = Instant::now() + Duration::from_millis(timeout_ms.clamp(100, 2000));
    let mut found: BTreeMap<String, DiscoveredServer> = BTreeMap::new();
    let mut buf = [0u8; 2048];

    while Instant::now() < deadline {
        let Ok((len, peer)) = socket.recv_from(&mut buf) else {
            continue;
        };
        let Ok(DiscoveryWirePacket::ServerAnnounce {
            server_id,
            server_port,
            hello,
        }) = serde_json::from_slice(&buf[..len])
        else {
            continue;
        };

        found.entry(server_id.clone()).or_insert(DiscoveredServer {
            server_id,
            host: peer.ip().to_string(),
            port: server_port,
            show_title: hello.show_title,
            protocol_version: hello.protocol_version,
            setup_mode_required: hello.setup_mode_required,
        });
    }

    Ok(found.into_values().collect())
}

#[derive(Debug)]
struct SelfSignedCertVerifier(Arc<rustls::crypto::CryptoProvider>);

impl SelfSignedCertVerifier {
    fn new() -> Arc<Self> {
        Arc::new(Self(Arc::new(rustls::crypto::ring::default_provider())))
    }
}

impl rustls::client::danger::ServerCertVerifier for SelfSignedCertVerifier {
    fn verify_server_cert(
        &self,
        _end_entity: &CertificateDer<'_>,
        _intermediates: &[CertificateDer<'_>],
        _server_name: &ServerName<'_>,
        _ocsp: &[u8],
        _now: UnixTime,
    ) -> Result<rustls::client::danger::ServerCertVerified, rustls::Error> {
        Ok(rustls::client::danger::ServerCertVerified::assertion())
    }

    fn verify_tls12_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &rustls::DigitallySignedStruct,
    ) -> Result<rustls::client::danger::HandshakeSignatureValid, rustls::Error> {
        rustls::crypto::verify_tls12_signature(
            message,
            cert,
            dss,
            &self.0.signature_verification_algorithms,
        )
    }

    fn verify_tls13_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &rustls::DigitallySignedStruct,
    ) -> Result<rustls::client::danger::HandshakeSignatureValid, rustls::Error> {
        rustls::crypto::verify_tls13_signature(
            message,
            cert,
            dss,
            &self.0.signature_verification_algorithms,
        )
    }

    fn supported_verify_schemes(&self) -> Vec<rustls::SignatureScheme> {
        self.0.signature_verification_algorithms.supported_schemes()
    }
}

pub type SessionRequest = (ClientPacket, oneshot::Sender<Result<ServerPacket, String>>);

pub struct ActiveQuicSession {
    pub tx: mpsc::UnboundedSender<SessionRequest>,
    pub stop_tx: watch::Sender<bool>,
}

use tokio::sync::watch;

async fn dial_quic_stream(
    addr_str: &str,
) -> Result<
    (
        Endpoint,
        quinn::Connection,
        quinn::SendStream,
        tokio::io::Lines<BufReader<quinn::RecvStream>>,
        ServerHello,
    ),
    String,
> {
    let _ = rustls::crypto::ring::default_provider().install_default();

    let remote = addr_str
        .to_socket_addrs()
        .map_err(|e| e.to_string())?
        .next()
        .ok_or("Could not resolve server address")?;

    let mut endpoint = Endpoint::client(SocketAddr::new(IpAddr::V4(Ipv4Addr::UNSPECIFIED), 0))
        .map_err(|e| e.to_string())?;

    let crypto = rustls::ClientConfig::builder()
        .dangerous()
        .with_custom_certificate_verifier(SelfSignedCertVerifier::new())
        .with_no_client_auth();
    let quic_crypto = QuicClientConfig::try_from(crypto).map_err(|e| e.to_string())?;
    let mut client_cfg = ClientConfig::new(Arc::new(quic_crypto));

    let mut transport = TransportConfig::default();
    transport.keep_alive_interval(Some(Duration::from_secs(5)));
    client_cfg.transport_config(Arc::new(transport));

    endpoint.set_default_client_config(client_cfg);

    let connecting = endpoint
        .connect(remote, "opengobo.local")
        .map_err(|e| e.to_string())?;
    let conn = tokio::time::timeout(Duration::from_millis(2000), connecting)
        .await
        .map_err(|_| "QUIC connection timed out".to_string())?
        .map_err(|e| e.to_string())?;

    let (send, recv) = conn.open_bi().await.map_err(|e| e.to_string())?;
    let mut lines = BufReader::new(recv).lines();

    let first_line = tokio::time::timeout(Duration::from_millis(2000), lines.next_line())
        .await
        .map_err(|_| "Timed out waiting for ServerHello".to_string())?
        .map_err(|e| e.to_string())?
        .ok_or("QUIC stream closed before ServerHello")?;

    let ServerPacket::Hello(hello) =
        serde_json::from_str::<ServerPacket>(&first_line).map_err(|e| e.to_string())?
    else {
        return Err("Expected ServerHello packet".to_string());
    };

    Ok((endpoint, conn, send, lines, hello))
}

pub async fn start_persistent_session<F>(
    server_address: String,
    mut on_event: F,
) -> Result<(ServerHello, ActiveQuicSession), String>
where
    F: FnMut(SessionEvent) + Send + 'static,
{
    let (mut endpoint, mut conn, mut send, mut lines, hello) =
        dial_quic_stream(&server_address).await?;

    let (req_tx, mut req_rx) = mpsc::unbounded_channel::<SessionRequest>();
    let (stop_tx, mut stop_rx) = watch::channel(false);

    tokio::spawn(async move {
        let mut active_token: Option<String> = None;

        loop {
            let disconnected = run_stream_loop(
                &mut send,
                &mut lines,
                &mut req_rx,
                &mut stop_rx,
                &mut active_token,
                &mut on_event,
            )
            .await;

            conn.close(0u32.into(), b"closing");
            endpoint.wait_idle().await;

            if !disconnected || *stop_rx.borrow() {
                break;
            }

            on_event(SessionEvent::ConnectionChanged {
                connected: false,
                hello: None,
            });

            // Auto-reconnect loop with session-token restoration
            loop {
                tokio::select! {
                    _ = tokio::time::sleep(Duration::from_millis(1500)) => {}
                    _ = stop_rx.changed() => {
                        if *stop_rx.borrow() {
                            return;
                        }
                    }
                }

                if let Ok((new_ep, new_conn, mut new_send, mut new_lines, new_hello)) =
                    dial_quic_stream(&server_address).await
                {
                    on_event(SessionEvent::ConnectionChanged {
                        connected: true,
                        hello: Some(new_hello),
                    });

                    if let Some(token) = active_token.clone() {
                        let pkt = ClientPacket::LoginToken {
                            session_token: token,
                        };
                        if write_client_packet(&mut new_send, &pkt).await.is_ok()
                            && let Ok(Some(line)) = new_lines.next_line().await
                            && let Ok(ServerPacket::AuthSuccess {
                                session_token,
                                snapshot,
                            }) = serde_json::from_str::<ServerPacket>(&line)
                        {
                            active_token = Some(session_token.clone());
                            on_event(SessionEvent::Reauthenticated {
                                session_token,
                                snapshot,
                            });
                        }
                    }

                    endpoint = new_ep;
                    conn = new_conn;
                    send = new_send;
                    lines = new_lines;
                    break;
                }
            }
        }
    });

    Ok((
        hello,
        ActiveQuicSession {
            tx: req_tx,
            stop_tx,
        },
    ))
}

pub enum SessionEvent {
    ConnectionChanged {
        connected: bool,
        hello: Option<ServerHello>,
    },
    Reauthenticated {
        session_token: String,
        snapshot: Box<ShowSnapshot>,
    },
    Delta(StateDelta),
}

async fn run_stream_loop<F>(
    send: &mut quinn::SendStream,
    lines: &mut tokio::io::Lines<BufReader<quinn::RecvStream>>,
    req_rx: &mut mpsc::UnboundedReceiver<SessionRequest>,
    stop_rx: &mut watch::Receiver<bool>,
    active_token: &mut Option<String>,
    on_event: &mut F,
) -> bool
where
    F: FnMut(SessionEvent),
{
    let mut pending_reply: Option<oneshot::Sender<Result<ServerPacket, String>>> = None;

    loop {
        tokio::select! {
            _ = stop_rx.changed() => {
                if *stop_rx.borrow() {
                    return false;
                }
            }
            maybe_req = req_rx.recv(), if pending_reply.is_none() => {
                let Some((packet, reply_tx)) = maybe_req else {
                    return false;
                };
                if matches!(packet, ClientPacket::Logout) {
                    *active_token = None;
                }
                if let Err(err) = write_client_packet(send, &packet).await {
                    let _ = reply_tx.send(Err(err));
                    return true;
                }
                pending_reply = Some(reply_tx);
            }
            line_res = lines.next_line() => {
                match line_res {
                    Ok(Some(line)) => {
                        let Ok(pkt) = serde_json::from_str::<ServerPacket>(line.trim()) else {
                            continue;
                        };
                        match pkt {
                            ServerPacket::Delta(delta) => {
                                on_event(SessionEvent::Delta(delta));
                            }
                            other => {
                                if let ServerPacket::AuthSuccess { ref session_token, .. } = other {
                                    *active_token = Some(session_token.clone());
                                }
                                if let Some(reply_tx) = pending_reply.take() {
                                    let _ = reply_tx.send(Ok(other));
                                }
                            }
                        }
                    }
                    _ => {
                        if let Some(reply_tx) = pending_reply.take() {
                            let _ = reply_tx.send(Err("QUIC stream disconnected".to_string()));
                        }
                        return true;
                    }
                }
            }
        }
    }
}

async fn write_client_packet(
    send: &mut quinn::SendStream,
    packet: &ClientPacket,
) -> Result<(), String> {
    let mut bytes = serde_json::to_vec(packet).map_err(|e| e.to_string())?;
    bytes.push(b'\n');
    send.write_all(&bytes).await.map_err(|e| e.to_string())
}

pub fn apply_delta_to_snapshot(snapshot: &mut ShowSnapshot, delta: StateDelta) {
    match delta {
        StateDelta::GrandMasterChanged { value } => {
            snapshot.grand_master = value;
        }
        StateDelta::BlackoutChanged { active } => {
            snapshot.blackout = active;
        }
        StateDelta::FixtureUpserted { fixture } => {
            snapshot.patch.retain(|f| f.id != fixture.id);
            snapshot.patch.push(fixture);
        }
        StateDelta::FixtureRemoved { fixture_id } => {
            snapshot.patch.retain(|f| f.id != fixture_id);
        }
        StateDelta::GroupUpserted { group } => {
            snapshot.groups.retain(|g| g.id != group.id);
            snapshot.groups.push(group);
        }
        StateDelta::GroupDeleted { group_id } => {
            snapshot.groups.retain(|g| g.id != group_id);
        }
        StateDelta::SceneUpserted { scene } => {
            snapshot.scenes.retain(|s| s.id != scene.id);
            snapshot.scenes.push(scene);
        }
        StateDelta::SceneDeleted { scene_id } => {
            snapshot.scenes.retain(|s| s.id != scene_id);
            snapshot.active_playbacks.remove(&scene_id);
        }
        StateDelta::ScenePlaybackChanged { scene_id, level } => {
            if level.get() <= 0.0 {
                snapshot.active_playbacks.remove(&scene_id);
            } else {
                snapshot.active_playbacks.insert(scene_id, level);
            }
        }
        StateDelta::ProgrammerUpdated {
            user_id,
            programmer,
        } => {
            if user_id == snapshot.user.id {
                snapshot.own_programmer = programmer;
            }
        }
    }
}
