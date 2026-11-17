use opengobo_protocol::{ClientPacket, ServerHello, ServerPacket};
use quinn::{ClientConfig, Endpoint};
use quinn::crypto::rustls::QuicClientConfig;
use rustls_pki_types::{CertificateDer, ServerName, UnixTime};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::net::{IpAddr, Ipv4Addr, SocketAddr, ToSocketAddrs, UdpSocket};
use std::sync::Arc;
use std::time::{Duration, Instant};

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

async fn open_quic_connection(addr_str: &str) -> Result<(Endpoint, quinn::Connection), String> {
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
    endpoint.set_default_client_config(ClientConfig::new(Arc::new(quic_crypto)));

    let connecting = endpoint
        .connect(remote, "opengobo.local")
        .map_err(|e| e.to_string())?;
    let conn = tokio::time::timeout(Duration::from_millis(2000), connecting)
        .await
        .map_err(|_| "QUIC connection timed out".to_string())?
        .map_err(|e| e.to_string())?;

    Ok((endpoint, conn))
}

pub async fn probe_server(host: &str, port: u16) -> Result<ServerHello, String> {
    let (endpoint, conn) = open_quic_connection(&format!("{host}:{port}")).await?;
    let (mut send, mut recv) = conn.open_bi().await.map_err(|e| e.to_string())?;
    let _ = send.finish();

    let raw = recv.read_to_end(65_536).await.map_err(|e| e.to_string())?;
    conn.close(0u32.into(), b"probe");
    endpoint.wait_idle().await;

    let first_line = raw.split(|&b| b == b'\n').next().unwrap_or(&raw);
    match serde_json::from_slice::<ServerPacket>(first_line).map_err(|e| e.to_string())? {
        ServerPacket::Hello(hello) => Ok(hello),
        _ => Err("Unexpected handshake packet from server".to_string()),
    }
}

pub async fn send_packet(
    server_address: &str,
    packet: &ClientPacket,
) -> Result<(ServerHello, ServerPacket), String> {
    let (endpoint, conn) = open_quic_connection(server_address).await?;
    let (mut send, mut recv) = conn.open_bi().await.map_err(|e| e.to_string())?;

    let mut payload = serde_json::to_vec(packet).map_err(|e| e.to_string())?;
    payload.push(b'\n');
    send.write_all(&payload).await.map_err(|e| e.to_string())?;
    send.finish().map_err(|e| e.to_string())?;

    let raw = recv.read_to_end(131_072).await.map_err(|e| e.to_string())?;
    conn.close(0u32.into(), b"done");
    endpoint.wait_idle().await;

    let mut lines = raw
        .split(|&b| b == b'\n')
        .filter(|line| !line.trim_ascii().is_empty());

    let hello_bytes = lines.next().ok_or("Missing ServerHello on QUIC stream")?;
    let ServerPacket::Hello(hello) =
        serde_json::from_slice::<ServerPacket>(hello_bytes).map_err(|e| e.to_string())?
    else {
        return Err("Expected ServerHello packet".to_string());
    };

    let resp_bytes = lines
        .next()
        .ok_or("Missing response packet on QUIC stream")?;
    let resp = serde_json::from_slice::<ServerPacket>(resp_bytes).map_err(|e| e.to_string())?;

    Ok((hello, resp))
}
