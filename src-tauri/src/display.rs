use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Manager, PhysicalPosition, WebviewUrl, WebviewWindowBuilder};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MonitorInfo {
    pub index: usize,
    pub name: String,
    pub width: u32,
    pub height: u32,
    pub x: i32,
    pub y: i32,
    pub is_primary: bool,
}

pub fn get_monitors(app: &AppHandle) -> Result<Vec<MonitorInfo>, String> {
    let available = app.available_monitors().map_err(|e| e.to_string())?;
    let primary_pos = app
        .primary_monitor()
        .ok()
        .flatten()
        .map(|m| (*m.position(), *m.size()));

    let monitors: Vec<MonitorInfo> = available
        .into_iter()
        .enumerate()
        .map(|(index, mon)| {
            let pos = *mon.position();
            let size = *mon.size();
            let raw_name = mon
                .name()
                .cloned()
                .unwrap_or_else(|| format!("Display {}", index + 1));
            let is_primary = primary_pos.is_some_and(|(p, s)| p == pos && s == size);

            MonitorInfo {
                index,
                name: format!("Monitor {} ({raw_name})", index + 1),
                width: size.width,
                height: size.height,
                x: pos.x,
                y: pos.y,
                is_primary,
            }
        })
        .collect();

    Ok(monitors)
}

fn close_screen_windows(app: &AppHandle) {
    for (label, win) in app.webview_windows() {
        if label.starts_with("screen-") {
            let _ = win.close();
        }
    }
}

pub fn start_screens(
    app: &AppHandle,
    selected_monitors: &[usize],
    fullscreen: bool,
) -> Result<(), String> {
    let monitors = get_monitors(app)?;
    let selected: Vec<&MonitorInfo> = selected_monitors
        .iter()
        .filter_map(|&i| monitors.get(i))
        .collect();

    if selected.is_empty() {
        return Err("Select at least one monitor".to_string());
    }

    close_screen_windows(app);

    for (i, mon) in selected.iter().enumerate() {
        let screen_num = i + 1;
        let label = format!("screen-{screen_num}");
        let url = WebviewUrl::App(format!("/?screen={screen_num}").into());

        let win = WebviewWindowBuilder::new(app, &label, url)
            .title(format!("OpenGobo - Screen {screen_num}"))
            .position(f64::from(mon.x), f64::from(mon.y))
            .inner_size(1280.0, 800.0)
            .decorations(!fullscreen)
            .always_on_top(true)
            .zoom_hotkeys_enabled(false)
            .disable_drag_drop_handler()
            .browser_extensions_enabled(false)
            .devtools(cfg!(debug_assertions))
            .build()
            .map_err(|e| e.to_string())?;

        if fullscreen {
            win.set_fullscreen_on_monitor(PhysicalPosition::new(
                f64::from(mon.x),
                f64::from(mon.y),
            ))
            .map_err(|e| e.to_string())?;
        }

        let _ = win.set_always_on_top(true);
    }

    if let Some(main_win) = app.get_webview_window("main") {
        let _ = main_win.hide();
    }

    Ok(())
}

pub fn show_launcher(app: &AppHandle) {
    close_screen_windows(app);
    if let Some(main_win) = app.get_webview_window("main") {
        let _ = main_win.show();
        let _ = main_win.set_focus();
    }
}
