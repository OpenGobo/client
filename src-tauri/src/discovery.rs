use opengobo_protocol::{ServerHello, ServerPacket};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::io::{BufRead, BufReader};
use std::net::{Ipv4Addr, SocketAddr, TcpStream, ToSocketAddrs, UdpSocket};
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

pub fn probe_server(host: &str, port: u16) -> Result<ServerHello, String> {
    let addr = format!("{host}:{port}")
        .to_socket_addrs()
        .map_err(|e| e.to_string())?
        .next()
        .ok_or("Could not resolve server address")?;

    let stream =
        TcpStream::connect_timeout(&addr, Duration::from_millis(1500)).map_err(|e| e.to_string())?;
    stream
        .set_read_timeout(Some(Duration::from_millis(1500)))
        .map_err(|e| e.to_string())?;

    let mut reader = BufReader::new(stream);
    let mut line = String::new();
    reader.read_line(&mut line).map_err(|e| e.to_string())?;

    match serde_json::from_str::<ServerPacket>(&line).map_err(|e| e.to_string())? {
        ServerPacket::Hello(hello) => Ok(hello),
        _ => Err("Unexpected handshake packet from server".to_string()),
    }
}
