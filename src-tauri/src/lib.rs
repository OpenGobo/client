pub mod display;

use display::MonitorInfo;
use opengobo_protocol::{ClientCommand, PROTOCOL_VERSION, ServerHello};
use serde::Serialize;
use tauri::AppHandle;

#[derive(Debug, Clone, Serialize)]
pub struct ClientRuntimeStatus {
    pub protocol_version: u32,
    pub connected: bool,
    pub server_hello: Option<ServerHello>,
}

#[tauri::command]
fn get_runtime_status() -> ClientRuntimeStatus {
    ClientRuntimeStatus {
        protocol_version: PROTOCOL_VERSION,
        connected: false,
        server_hello: None,
    }
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
        .plugin(tauri_plugin_opener::init())
        .invoke_handler(tauri::generate_handler![
            get_runtime_status,
            validate_command,
            list_monitors,
            launch_screens,
            reopen_launcher
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
