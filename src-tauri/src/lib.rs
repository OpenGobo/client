pub mod discovery;
pub mod display;

use discovery::DiscoveredServer;
use display::MonitorInfo;
use opengobo_protocol::{
    ClientCommand, ClientPacket, PROTOCOL_VERSION, ServerHello, ServerPacket, ShowSnapshot,
};
use serde::Serialize;
use std::sync::Mutex;
use tauri::{AppHandle, Emitter, State};

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

fn update_and_emit(
    app: &AppHandle,
    state: &State<'_, Mutex<ClientRuntimeStatus>>,
    next: ClientRuntimeStatus,
) -> ClientRuntimeStatus {
    if let Ok(mut guard) = state.lock() {
        *guard = next.clone();
    }
    let _ = app.emit("runtime-status", &next);
    next
}

#[tauri::command]
fn get_runtime_status(state: State<'_, Mutex<ClientRuntimeStatus>>) -> ClientRuntimeStatus {
    state.lock().map(|s| s.clone()).unwrap_or_default()
}

#[tauri::command]
fn discover_servers() -> Result<Vec<DiscoveredServer>, String> {
    discovery::scan_lan(300)
}

#[tauri::command]
fn connect_server(
    app: AppHandle,
    host: String,
    port: u16,
    state: State<'_, Mutex<ClientRuntimeStatus>>,
) -> Result<ClientRuntimeStatus, String> {
    let hello = discovery::probe_server(&host, port)?;
    let next = ClientRuntimeStatus {
        protocol_version: PROTOCOL_VERSION,
        connected: true,
        server_address: Some(format!("{host}:{port}")),
        server_hello: Some(hello),
        session_token: None,
        snapshot: None,
    };
    Ok(update_and_emit(&app, &state, next))
}

#[tauri::command]
fn complete_setup(
    app: AppHandle,
    setup_pin: String,
    admin_username: String,
    admin_password: String,
    state: State<'_, Mutex<ClientRuntimeStatus>>,
) -> Result<ClientRuntimeStatus, String> {
    let current = get_runtime_status(state.clone());
    let addr = current
        .server_address
        .clone()
        .ok_or("Not connected to a Core server")?;

    let packet = ClientPacket::CompleteSetup {
        setup_pin,
        admin_username,
        admin_password,
    };
    let (mut hello, resp) = discovery::send_packet(&addr, &packet)?;

    match resp {
        ServerPacket::AuthSuccess {
            session_token,
            snapshot,
        } => {
            hello.setup_mode_required = false;
            let next = ClientRuntimeStatus {
                protocol_version: PROTOCOL_VERSION,
                connected: true,
                server_address: Some(addr),
                server_hello: Some(hello),
                session_token: Some(session_token),
                snapshot: Some(*snapshot),
            };
            Ok(update_and_emit(&app, &state, next))
        }
        ServerPacket::AuthError { message } => Err(message),
        _ => Err("Unexpected response from Core server".to_string()),
    }
}

#[tauri::command]
fn login_password(
    app: AppHandle,
    username: String,
    password: String,
    state: State<'_, Mutex<ClientRuntimeStatus>>,
) -> Result<ClientRuntimeStatus, String> {
    let current = get_runtime_status(state.clone());
    let addr = current
        .server_address
        .clone()
        .ok_or("Not connected to a Core server")?;

    let packet = ClientPacket::LoginPassword { username, password };
    let (hello, resp) = discovery::send_packet(&addr, &packet)?;

    match resp {
        ServerPacket::AuthSuccess {
            session_token,
            snapshot,
        } => {
            let next = ClientRuntimeStatus {
                protocol_version: PROTOCOL_VERSION,
                connected: true,
                server_address: Some(addr),
                server_hello: Some(hello),
                session_token: Some(session_token),
                snapshot: Some(*snapshot),
            };
            Ok(update_and_emit(&app, &state, next))
        }
        ServerPacket::AuthError { message } => Err(message),
        _ => Err("Unexpected response from Core server".to_string()),
    }
}

#[tauri::command]
fn logout_user(
    app: AppHandle,
    state: State<'_, Mutex<ClientRuntimeStatus>>,
) -> ClientRuntimeStatus {
    let mut current = get_runtime_status(state.clone());
    if let Some(addr) = &current.server_address {
        let _ = discovery::send_packet(addr, &ClientPacket::Logout);
    }
    current.session_token = None;
    current.snapshot = None;
    update_and_emit(&app, &state, current)
}

#[tauri::command]
fn disconnect_server(
    app: AppHandle,
    state: State<'_, Mutex<ClientRuntimeStatus>>,
) -> ClientRuntimeStatus {
    update_and_emit(&app, &state, ClientRuntimeStatus::default())
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
        .manage(Mutex::new(ClientRuntimeStatus::default()))
        .plugin(tauri_plugin_opener::init())
        .invoke_handler(tauri::generate_handler![
            get_runtime_status,
            discover_servers,
            connect_server,
            complete_setup,
            login_password,
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
