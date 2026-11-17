pub mod discovery;
pub mod display;

use discovery::DiscoveredServer;
use display::MonitorInfo;
use opengobo_protocol::{ClientCommand, PROTOCOL_VERSION, ServerHello};
use serde::Serialize;
use std::sync::Mutex;
use tauri::{AppHandle, State};

#[derive(Debug, Clone, Serialize)]
pub struct ClientRuntimeStatus {
    pub protocol_version: u32,
    pub connected: bool,
    pub server_address: Option<String>,
    pub server_hello: Option<ServerHello>,
}

impl Default for ClientRuntimeStatus {
    fn default() -> Self {
        Self {
            protocol_version: PROTOCOL_VERSION,
            connected: false,
            server_address: None,
            server_hello: None,
        }
    }
}

#[tauri::command]
fn get_runtime_status(state: State<'_, Mutex<ClientRuntimeStatus>>) -> ClientRuntimeStatus {
    state
        .lock()
        .map(|s| s.clone())
        .unwrap_or_default()
}

#[tauri::command]
fn discover_servers() -> Result<Vec<DiscoveredServer>, String> {
    discovery::scan_lan(300)
}

#[tauri::command]
fn connect_server(
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
    };
    if let Ok(mut guard) = state.lock() {
        *guard = next.clone();
    }
    Ok(next)
}

#[tauri::command]
fn disconnect_server(state: State<'_, Mutex<ClientRuntimeStatus>>) -> ClientRuntimeStatus {
    let next = ClientRuntimeStatus::default();
    if let Ok(mut guard) = state.lock() {
        *guard = next.clone();
    }
    next
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
            disconnect_server,
            validate_command,
            list_monitors,
            launch_screens,
            reopen_launcher
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
