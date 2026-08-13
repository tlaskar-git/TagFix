// TagFix: tag what is wrong on screen, get a fix list out.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::sync::Mutex;

use tauri::menu::{Menu, MenuItem};
use tauri::tray::TrayIconBuilder;
use tauri::{AppHandle, Emitter, Manager, State, WebviewUrl, WebviewWindowBuilder};
use tauri_plugin_global_shortcut::{Code, GlobalShortcutExt, Modifiers, Shortcut, ShortcutState};

struct AppState {
    armed: Mutex<bool>,
}

fn sweeps_dir() -> std::path::PathBuf {
    let exe_dir = std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|d| d.to_path_buf()))
        .unwrap_or_else(|| std::path::PathBuf::from("."));
    exe_dir.join("sweeps")
}

fn apply_armed(app: &AppHandle, armed: bool) {
    let state: State<AppState> = app.state();
    *state.armed.lock().unwrap() = armed;

    if let Some(win) = app.get_webview_window("overlay") {
        // Disarmed: the overlay must not intercept any clicks.
        let _ = win.set_ignore_cursor_events(!armed);
        if armed {
            let _ = win.show();
            let _ = win.set_focus();
        }
    }
    let _ = app.emit("armed-changed", armed);
}

fn toggle_armed(app: &AppHandle) {
    let armed = {
        let state: State<AppState> = app.state();
        let v = *state.armed.lock().unwrap();
        !v
    };
    apply_armed(app, armed);
}

#[tauri::command]
fn set_armed(app: AppHandle, armed: bool) {
    apply_armed(&app, armed);
}

#[tauri::command]
fn get_armed(state: State<AppState>) -> bool {
    *state.armed.lock().unwrap()
}

fn open_sweeps_folder() {
    let dir = sweeps_dir();
    let _ = std::fs::create_dir_all(&dir);
    let _ = std::process::Command::new("explorer.exe").arg(&dir).spawn();
}

fn build_overlay(app: &AppHandle) -> tauri::Result<()> {
    let win = WebviewWindowBuilder::new(app, "overlay", WebviewUrl::App("index.html".into()))
        .title("TagFix overlay")
        .transparent(true)
        .decorations(false)
        .always_on_top(true)
        .skip_taskbar(true)
        .shadow(false)
        .visible(true)
        .focused(false)
        .build()?;

    // Cover the primary monitor edge to edge.
    if let Ok(Some(monitor)) = win.primary_monitor() {
        let _ = win.set_position(monitor.position().clone());
        let _ = win.set_size(monitor.size().clone());
    }
    // Start disarmed: click-through.
    let _ = win.set_ignore_cursor_events(true);
    Ok(())
}

fn main() {
    tauri::Builder::default()
        .manage(AppState {
            armed: Mutex::new(false),
        })
        .plugin(
            tauri_plugin_global_shortcut::Builder::new()
                .with_handler(|app, _shortcut, event| {
                    if event.state() == ShortcutState::Pressed {
                        toggle_armed(app);
                    }
                })
                .build(),
        )
        .invoke_handler(tauri::generate_handler![set_armed, get_armed])
        .setup(|app| {
            let handle = app.handle().clone();
            build_overlay(&handle)?;

            // Global hotkey: Ctrl+Shift+T toggles armed state.
            let hotkey = Shortcut::new(Some(Modifiers::CONTROL | Modifiers::SHIFT), Code::KeyT);
            app.global_shortcut().register(hotkey)?;

            // Tray icon and menu.
            let arm_item = MenuItem::with_id(app, "arm", "Arm (Ctrl+Shift+T)", true, None::<&str>)?;
            let sweeps_item =
                MenuItem::with_id(app, "open-sweeps", "Open sweeps folder", true, None::<&str>)?;
            let settings_item =
                MenuItem::with_id(app, "settings", "Settings", true, None::<&str>)?;
            let quit_item = MenuItem::with_id(app, "quit", "Quit", true, None::<&str>)?;
            let menu = Menu::with_items(
                app,
                &[&arm_item, &sweeps_item, &settings_item, &quit_item],
            )?;

            TrayIconBuilder::with_id("main")
                .icon(app.default_window_icon().unwrap().clone())
                .tooltip("TagFix")
                .menu(&menu)
                .on_menu_event(|app, event| match event.id().as_ref() {
                    "arm" => toggle_armed(app),
                    "open-sweeps" => open_sweeps_folder(),
                    "settings" => {
                        // Settings window ships in Phase 6.
                    }
                    "quit" => app.exit(0),
                    _ => {}
                })
                .build(app)?;

            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running TagFix");
}
