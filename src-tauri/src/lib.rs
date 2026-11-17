pub mod discovery;
pub mod display;

use discovery::{ActiveQuicSession, DiscoveredServer, SessionEvent, apply_delta_to_snapshot};
use display::MonitorInfo;
use opengobo_protocol::{
    ClientCommand, ClientPacket, PROTOCOL_VERSION, ServerHello, ServerPacket, ShowSnapshot,
};
use serde::Serialize;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tauri::{AppHandle, Emitter, State};
use tokio::sync::oneshot;

#[derive(Debug, Clone, Serialize)]
pub struct ClientRuntimeStatus {
    pub protocol_version: u32,
    pub connected: bool,
    pub server_address: Option<String>,
    pub server_hello: Option<ServerHello>,
    pub session_token: Option<String>,
    pub snapshot: Option<ShowSnapshot>,
}

impl Default for ClientRuntimeStatus {
    fn default() -> Self {
        Self {
            protocol_version: PROTOCOL_VERSION,
            connected: false,
            server_address: None,
            server_hello: None,
            session_token: None,
            snapshot: None,
        }
    }
}

#[derive(Default)]
pub struct ClientManager {
    pub status: Arc<Mutex<ClientRuntimeStatus>>,
    pub session: Mutex<Option<ActiveQuicSession>>,
    pub next_request_id: Mutex<u64>,
}

fn read_status(mgr: &ClientManager) -> ClientRuntimeStatus {
    mgr.status.lock().map(|s| s.clone()).unwrap_or_default()
}

fn set_status_and_emit(
    app: &AppHandle,
    status_arc: &Arc<Mutex<ClientRuntimeStatus>>,
    next: ClientRuntimeStatus,
) -> ClientRuntimeStatus {
    if let Ok(mut guard) = status_arc.lock() {
        *guard = next.clone();
    }
    let _ = app.emit("runtime-status", &next);
    next
}

async fn send_over_session(
    mgr: &State<'_, ClientManager>,
    packet: ClientPacket,
) -> Result<ServerPacket, String> {
    let tx = mgr
        .session
        .lock()
        .map_err(|_| "Session state lock poisoned".to_string())?
        .as_ref()
        .map(|s| s.tx.clone())
        .ok_or("No active QUIC session")?;

    let (reply_tx, reply_rx) = oneshot::channel();
    tx.send((packet, reply_tx))
        .map_err(|_| "QUIC session channel closed".to_string())?;

    tokio::time::timeout(Duration::from_millis(3000), reply_rx)
        .await
        .map_err(|_| "Timed out waiting for Core response".to_string())?
        .map_err(|_| "QUIC response channel dropped".to_string())?
}

#[tauri::command]
fn get_runtime_status(mgr: State<'_, ClientManager>) -> ClientRuntimeStatus {
    read_status(&mgr)
}

#[tauri::command]
fn discover_servers() -> Result<Vec<DiscoveredServer>, String> {
    discovery::scan_lan(300)
}

#[tauri::command]
async fn connect_server(
    app: AppHandle,
    host: String,
    port: u16,
    mgr: State<'_, ClientManager>,
) -> Result<ClientRuntimeStatus, String> {
    if let Ok(mut old) = mgr.session.lock()
        && let Some(prev) = old.take()
    {
        let _ = prev.stop_tx.send(true);
    }

    let addr = format!("{host}:{port}");
    let status_arc = Arc::clone(&mgr.status);
    let app_clone = app.clone();

    let (hello, session) = discovery::start_persistent_session(addr.clone(), move |ev| {
        let mut current = status_arc.lock().map(|s| s.clone()).unwrap_or_default();
        match ev {
            SessionEvent::ConnectionChanged { connected, hello } => {
                current.connected = connected;
                if let Some(h) = hello {
                    current.server_hello = Some(h);
                }
            }
            SessionEvent::Reauthenticated {
                session_token,
                snapshot,
            } => {
                current.connected = true;
                current.session_token = Some(session_token);
                current.snapshot = Some(*snapshot);
            }
            SessionEvent::Delta(delta) => {
                if let Some(ref mut snap) = current.snapshot {
                    apply_delta_to_snapshot(snap, delta);
                }
            }
        }
        set_status_and_emit(&app_clone, &status_arc, current);
    })
    .await?;

    if let Ok(mut guard) = mgr.session.lock() {
        *guard = Some(session);
    }

    let next = ClientRuntimeStatus {
        protocol_version: PROTOCOL_VERSION,
        connected: true,
        server_address: Some(addr),
        server_hello: Some(hello),
        session_token: None,
        snapshot: None,
    };
    Ok(set_status_and_emit(&app, &mgr.status, next))
}

#[tauri::command]
async fn complete_setup(
    app: AppHandle,
    setup_pin: String,
    admin_username: String,
    admin_password: String,
    mgr: State<'_, ClientManager>,
) -> Result<ClientRuntimeStatus, String> {
    let packet = ClientPacket::CompleteSetup {
        setup_pin,
        admin_username,
        admin_password,
    };
    let resp = send_over_session(&mgr, packet).await?;

    match resp {
        ServerPacket::AuthSuccess {
            session_token,
            snapshot,
        } => {
            let mut next = read_status(&mgr);
            if let Some(ref mut h) = next.server_hello {
                h.setup_mode_required = false;
            }
            next.session_token = Some(session_token);
            next.snapshot = Some(*snapshot);
            Ok(set_status_and_emit(&app, &mgr.status, next))
        }
        ServerPacket::AuthError { message } => Err(message),
        _ => Err("Unexpected response from Core server".to_string()),
    }
}

#[tauri::command]
async fn login_password(
    app: AppHandle,
    username: String,
    password: String,
    mgr: State<'_, ClientManager>,
) -> Result<ClientRuntimeStatus, String> {
    let packet = ClientPacket::LoginPassword { username, password };
    let resp = send_over_session(&mgr, packet).await?;

    match resp {
        ServerPacket::AuthSuccess {
            session_token,
            snapshot,
        } => {
            let mut next = read_status(&mgr);
            next.session_token = Some(session_token);
            next.snapshot = Some(*snapshot);
            Ok(set_status_and_emit(&app, &mgr.status, next))
        }
        ServerPacket::AuthError { message } => Err(message),
        _ => Err("Unexpected response from Core server".to_string()),
    }
}

#[tauri::command]
async fn send_command(
    command: ClientCommand,
    mgr: State<'_, ClientManager>,
) -> Result<(), String> {
    let request_id = {
        let mut id_guard = mgr
            .next_request_id
            .lock()
            .map_err(|_| "Lock poisoned".to_string())?;
        *id_guard = id_guard.saturating_add(1);
        *id_guard
    };

    let resp = send_over_session(
        &mgr,
        ClientPacket::Command {
            request_id,
            command,
        },
    )
    .await?;

    match resp {
        ServerPacket::CommandResponse {
            accepted: true, ..
        } => Ok(()),
        ServerPacket::CommandResponse {
            accepted: false,
            error,
            ..
        } => Err(error.unwrap_or_else(|| "Command rejected".to_string())),
        _ => Err("Unexpected command response".to_string()),
    }
}

#[tauri::command]
async fn logout_user(
    app: AppHandle,
    mgr: State<'_, ClientManager>,
) -> Result<ClientRuntimeStatus, String> {
    let _ = send_over_session(&mgr, ClientPacket::Logout).await;
    let mut current = read_status(&mgr);
    current.session_token = None;
    current.snapshot = None;
    Ok(set_status_and_emit(&app, &mgr.status, current))
}

#[tauri::command]
fn disconnect_server(
    app: AppHandle,
    mgr: State<'_, ClientManager>,
) -> ClientRuntimeStatus {
    if let Ok(mut old) = mgr.session.lock()
        && let Some(prev) = old.take()
    {
        let _ = prev.stop_tx.send(true);
    }
    set_status_and_emit(&app, &mgr.status, ClientRuntimeStatus::default())
}

#[tauri::command]
fn validate_command(command: ClientCommand) -> Result<ClientCommand, String> {
    Ok(command)
}

#[tauri::command]
fn list_monitors(app: AppHandle) -> Result<Vec<MonitorInfo>, String> {
    display::get_monitors(&app)
}

#[tauri::command]
fn launch_screens(
    app: AppHandle,
    selected_monitors: Vec<usize>,
    fullscreen: bool,
) -> Result<(), String> {
    display::start_screens(&app, &selected_monitors, fullscreen)
}

#[tauri::command]
fn reopen_launcher(app: AppHandle) {
    display::show_launcher(&app);
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .manage(ClientManager::default())
        .plugin(tauri_plugin_opener::init())
        .invoke_handler(tauri::generate_handler![
            get_runtime_status,
            discover_servers,
            connect_server,
            complete_setup,
            login_password,
            send_command,
            logout_user,
            disconnect_server,
            validate_command,
            list_monitors,
            launch_screens,
            reopen_launcher
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
