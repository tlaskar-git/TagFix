// TagFix: tag what is wrong on screen, get a fix list out.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod capture;

use std::sync::Mutex;

use serde::Serialize;
use tauri::menu::{Menu, MenuItem};
use tauri::tray::TrayIconBuilder;
use tauri::{AppHandle, Emitter, Manager, State, WebviewUrl, WebviewWindowBuilder};
use tauri_plugin_global_shortcut::{Code, GlobalShortcutExt, Modifiers, Shortcut, ShortcutState};

use tagfix::settings::{self, Settings};
use tagfix::store::{self, SweepStore, Tag};

/// Everything remembered at arm time, so a capture can be attributed to the
/// monitor and foreground app the operator was actually looking at.
#[derive(Clone)]
struct ArmContext {
    monitor_index: u32,
    monitor_name: String,
    dpi_scale: f64,
    monitor_x: i32,
    monitor_y: i32,
    monitor_w: u32,
    monitor_h: u32,
    window_title: String,
    process_name: String,
}

/// A captured region whose PNG exists on disk but whose tag has not been
/// saved into sweep.json yet. Esc throws it away, Enter persists it.
struct PendingTag {
    sweep_name: String,
    tag: Tag,
}

struct AppState {
    armed: Mutex<bool>,
    arm_ctx: Mutex<Option<ArmContext>>,
    pending: Mutex<Option<PendingTag>>,
    hotkey: Mutex<String>,
}

fn now_utc() -> String {
    chrono::Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string()
}

fn exe_dir() -> std::path::PathBuf {
    std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|d| d.to_path_buf()))
        .unwrap_or_else(|| std::path::PathBuf::from("."))
}

fn sweeps_dir() -> std::path::PathBuf {
    let dir = exe_dir();
    settings::resolve_sweeps_dir(&dir, &settings::load(&dir))
}

/// Parse an operator supplied hotkey string, falling back to the default.
fn parse_hotkey(raw: &str) -> Shortcut {
    raw.parse::<Shortcut>().unwrap_or_else(|_| {
        Shortcut::new(Some(Modifiers::CONTROL | Modifiers::SHIFT), Code::KeyT)
    })
}

/// Launch at login via the per-user Run key. No admin rights involved.
fn apply_launch_at_login(enable: bool) {
    let run_key = r"HKCU\Software\Microsoft\Windows\CurrentVersion\Run";
    if enable {
        if let Ok(exe) = std::env::current_exe() {
            let _ = std::process::Command::new("reg")
                .args([
                    "add",
                    run_key,
                    "/v",
                    "TagFix",
                    "/t",
                    "REG_SZ",
                    "/d",
                    &exe.display().to_string(),
                    "/f",
                ])
                .output();
        }
    } else {
        let _ = std::process::Command::new("reg")
            .args(["delete", run_key, "/v", "TagFix", "/f"])
            .output();
    }
}

/// Move the overlay onto the monitor that currently hosts the cursor and
/// remember that monitor plus the foreground app for capture metadata.
fn prepare_arm_context(app: &AppHandle) -> Option<ArmContext> {
    let win = app.get_webview_window("overlay")?;
    let cursor = app.cursor_position().ok();
    let monitors = win.available_monitors().ok()?;

    let mut chosen = None;
    if let Some(cur) = cursor {
        for (i, m) in monitors.iter().enumerate() {
            let p = m.position();
            let s = m.size();
            let inside_x = cur.x >= p.x as f64 && cur.x < (p.x + s.width as i32) as f64;
            let inside_y = cur.y >= p.y as f64 && cur.y < (p.y + s.height as i32) as f64;
            if inside_x && inside_y {
                chosen = Some((i, m.clone()));
                break;
            }
        }
    }
    let (index, monitor) = match chosen {
        Some(v) => v,
        None => (0, win.primary_monitor().ok()??),
    };

    // Grab the foreground app BEFORE the overlay takes focus.
    let fg = capture::foreground_info();

    let _ = win.set_position(monitor.position().clone());
    let _ = win.set_size(monitor.size().clone());

    Some(ArmContext {
        monitor_index: index as u32,
        monitor_name: monitor.name().cloned().unwrap_or_default(),
        dpi_scale: monitor.scale_factor(),
        monitor_x: monitor.position().x,
        monitor_y: monitor.position().y,
        monitor_w: monitor.size().width,
        monitor_h: monitor.size().height,
        window_title: fg.window_title,
        process_name: fg.process_name,
    })
}

fn apply_armed(app: &AppHandle, armed: bool) {
    let state: State<AppState> = app.state();
    *state.armed.lock().unwrap() = armed;

    if armed {
        let ctx = prepare_arm_context(app);
        *state.arm_ctx.lock().unwrap() = ctx;
    }

    if let Some(win) = app.get_webview_window("overlay") {
        // Disarmed: the overlay must not intercept any clicks.
        let _ = win.set_ignore_cursor_events(!armed);
        if armed {
            let _ = win.show();
            let _ = win.set_focus();
        }
    }

    // Esc must disarm even when another app holds keyboard focus, so it is
    // a global shortcut that only exists while armed.
    let esc = Shortcut::new(None, Code::Escape);
    if armed {
        let _ = app.global_shortcut().register(esc);
    } else {
        let _ = app.global_shortcut().unregister(esc);
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

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct CaptureResult {
    tag_number: u32,
    sweep_name: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct StatusResult {
    sweep_name: String,
    next_tag_number: u32,
}

#[tauri::command]
fn get_status() -> Result<StatusResult, String> {
    let store = SweepStore::new(sweeps_dir());
    let (sweep_name, sweep) = store.active_sweep(&now_utc()).map_err(|e| e.to_string())?;
    Ok(StatusResult {
        next_tag_number: sweep.next_tag_number(),
        sweep_name,
    })
}

/// Persist the pending tag with the operator's text and chips.
#[tauri::command]
fn save_tag(
    app: AppHandle,
    text: String,
    severity: String,
    area: String,
) -> Result<CaptureResult, String> {
    let pending = {
        let state: State<AppState> = app.state();
        let mut guard = state.pending.lock().unwrap();
        guard.take().ok_or("no pending tag to save")?
    };
    let mut tag = pending.tag;
    tag.text = text;
    tag.severity = severity;
    tag.area = area;
    let number = tag.number;

    let store = SweepStore::new(sweeps_dir());
    store
        .append_tag(&pending.sweep_name, tag)
        .map_err(|e| e.to_string())?;
    Ok(CaptureResult {
        tag_number: number,
        sweep_name: pending.sweep_name,
    })
}

/// Drop the pending tag and its PNG. Safe to call twice.
#[tauri::command]
fn cancel_tag(app: AppHandle) -> Result<(), String> {
    let pending = {
        let state: State<AppState> = app.state();
        let mut guard = state.pending.lock().unwrap();
        guard.take()
    };
    if let Some(p) = pending {
        let png = sweeps_dir().join(&p.sweep_name).join(&p.tag.image);
        let _ = std::fs::remove_file(png);
    }
    Ok(())
}

fn has_pending(app: &AppHandle) -> bool {
    let state: State<AppState> = app.state();
    let guard = state.pending.lock().unwrap();
    guard.is_some()
}

/// x, y, w, h arrive in overlay CSS pixels, relative to the armed monitor.
#[tauri::command]
async fn capture_region(
    app: AppHandle,
    x: f64,
    y: f64,
    w: f64,
    h: f64,
) -> Result<CaptureResult, String> {
    let ctx = {
        let state: State<AppState> = app.state();
        let guard = state.arm_ctx.lock().unwrap();
        guard.clone().ok_or("not armed, no capture context")?
    };

    let scale = ctx.dpi_scale;
    let px = (x * scale).round().max(0.0) as u32;
    let py = (y * scale).round().max(0.0) as u32;
    let pw = (w * scale).round() as u32;
    let ph = (h * scale).round() as u32;
    if pw < 4 || ph < 4 {
        return Err("selection too small".into());
    }

    let region = capture::MonitorRegion {
        x: px,
        y: py,
        width: pw,
        height: ph,
    };

    let ts = now_utc();
    let store = SweepStore::new(sweeps_dir());
    let (sweep_name, sweep) = store.active_sweep(&ts).map_err(|e| e.to_string())?;
    let number = sweep.next_tag_number();
    let image = store::tag_image_name(number);
    let out_path = store.root().join(&sweep_name).join(&image);

    // Give the compositor a beat to hide the overlay chrome the JS side
    // just switched off, so the capture does not contain our own UI.
    std::thread::sleep(std::time::Duration::from_millis(90));

    let monitor_name = ctx.monitor_name.clone();
    let capture_path = out_path.clone();
    tauri::async_runtime::spawn_blocking(move || {
        capture::capture_region_png(&monitor_name, region, capture_path)
    })
    .await
    .map_err(|e| e.to_string())??;

    let tag = Tag {
        number,
        image,
        captured_utc: ts,
        monitor_index: ctx.monitor_index,
        dpi_scale: ctx.dpi_scale,
        region: store::Rect {
            x: ctx.monitor_x + px as i32,
            y: ctx.monitor_y + py as i32,
            width: pw,
            height: ph,
        },
        window_title: ctx.window_title.clone(),
        process_name: ctx.process_name.clone(),
        screen_resolution: format!("{}x{}", ctx.monitor_w, ctx.monitor_h),
        text: String::new(),
        severity: String::new(),
        area: String::new(),
        dropped: false,
    };

    // The PNG is on disk but the tag is only pending: Enter saves it into
    // sweep.json, Esc deletes the PNG again.
    {
        let state: State<AppState> = app.state();
        *state.pending.lock().unwrap() = Some(PendingTag {
            sweep_name: sweep_name.clone(),
            tag,
        });
    }

    Ok(CaptureResult {
        tag_number: number,
        sweep_name,
    })
}

#[tauri::command]
fn list_sweeps() -> Result<Vec<(String, usize)>, String> {
    SweepStore::new(sweeps_dir())
        .list_sweeps()
        .map_err(|e| e.to_string())
}

#[tauri::command]
fn load_sweep(dir_name: String) -> Result<tagfix::store::Sweep, String> {
    SweepStore::new(sweeps_dir())
        .load_sweep(&dir_name)
        .map_err(|e| e.to_string())
}

#[tauri::command]
fn update_tag(
    dir_name: String,
    number: u32,
    text: String,
    severity: String,
    area: String,
) -> Result<(), String> {
    SweepStore::new(sweeps_dir())
        .update_tag(&dir_name, number, &text, &severity, &area)
        .map(|_| ())
        .map_err(|e| e.to_string())
}

#[tauri::command]
fn set_dropped(dir_name: String, number: u32, dropped: bool) -> Result<(), String> {
    SweepStore::new(sweeps_dir())
        .set_dropped(&dir_name, number, dropped)
        .map(|_| ())
        .map_err(|e| e.to_string())
}

#[tauri::command]
fn reorder_tags(dir_name: String, order: Vec<u32>) -> Result<(), String> {
    SweepStore::new(sweeps_dir())
        .reorder_tags(&dir_name, &order)
        .map(|_| ())
        .map_err(|e| e.to_string())
}

/// Write fixlist.md, fixlist.html and brief.md into the sweep folder and
/// return the one line pointer that goes to the clipboard. Nothing touches
/// disk until the operator presses Export, which calls this.
#[tauri::command]
fn export_sweep(dir_name: String) -> Result<String, String> {
    tagfix::export::export_sweep_files(&sweeps_dir(), &dir_name).map_err(|e| e.to_string())
}

#[tauri::command]
fn get_settings() -> Settings {
    settings::load(&exe_dir())
}

#[tauri::command]
fn save_settings(app: AppHandle, new_settings: Settings) -> Result<(), String> {
    // Validate the hotkey before anything is persisted.
    let parsed = new_settings
        .hotkey
        .parse::<Shortcut>()
        .map_err(|e| format!("invalid hotkey \"{}\": {}", new_settings.hotkey, e))?;

    let dir = exe_dir();
    let old = settings::load(&dir);
    settings::save(&dir, &new_settings).map_err(|e| e.to_string())?;

    if old.hotkey != new_settings.hotkey {
        let state: State<AppState> = app.state();
        let old_shortcut = parse_hotkey(&state.hotkey.lock().unwrap());
        let _ = app.global_shortcut().unregister(old_shortcut);
        app.global_shortcut()
            .register(parsed)
            .map_err(|e| e.to_string())?;
        *state.hotkey.lock().unwrap() = new_settings.hotkey.clone();
    }
    if old.launch_at_login != new_settings.launch_at_login {
        apply_launch_at_login(new_settings.launch_at_login);
    }
    Ok(())
}

fn open_settings(app: &AppHandle) {
    if let Some(w) = app.get_webview_window("settings") {
        let _ = w.show();
        let _ = w.set_focus();
        return;
    }
    let _ = WebviewWindowBuilder::new(app, "settings", WebviewUrl::App("settings.html".into()))
        .title("TagFix settings")
        .inner_size(460.0, 380.0)
        .build();
}

fn open_review(app: &AppHandle) {
    if let Some(w) = app.get_webview_window("review") {
        let _ = w.show();
        let _ = w.set_focus();
        return;
    }
    let _ = WebviewWindowBuilder::new(app, "review", WebviewUrl::App("review.html".into()))
        .title("TagFix review")
        .inner_size(980.0, 720.0)
        .build();
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

    if let Ok(Some(monitor)) = win.primary_monitor() {
        let _ = win.set_position(monitor.position().clone());
        let _ = win.set_size(monitor.size().clone());
    }
    // Start disarmed: click-through.
    let _ = win.set_ignore_cursor_events(true);
    Ok(())
}

/// `tagfix sweep new <slug>` and `tagfix sweep list`. Returns the process
/// exit code.
fn run_sweep_cli(rest: &[String]) -> i32 {
    let store = SweepStore::new(sweeps_dir());
    match rest.first().map(|s| s.as_str()) {
        Some("new") => {
            let Some(slug) = rest.get(1) else {
                eprintln!("usage: tagfix sweep new <slug>");
                return 1;
            };
            match store.create_sweep(slug, &now_utc()) {
                Ok((name, _)) => {
                    println!("created sweep {}", name);
                    0
                }
                Err(e) => {
                    eprintln!("error: {}", e);
                    1
                }
            }
        }
        Some("list") => match store.list_sweeps() {
            Ok(sweeps) => {
                if sweeps.is_empty() {
                    println!("no sweeps in {}", store.root().display());
                } else {
                    for (name, count) in sweeps {
                        println!("{}  ({} tags)", name, count);
                    }
                }
                0
            }
            Err(e) => {
                eprintln!("error: {}", e);
                1
            }
        },
        _ => {
            eprintln!("usage: tagfix sweep <new|list>");
            1
        }
    }
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() >= 2 && args[1] == "sweep" {
        // The release binary is a GUI app; borrow the parent console so the
        // CLI output is visible.
        #[cfg(not(debug_assertions))]
        unsafe {
            use windows::Win32::System::Console::{AttachConsole, ATTACH_PARENT_PROCESS};
            let _ = AttachConsole(ATTACH_PARENT_PROCESS);
        }
        std::process::exit(run_sweep_cli(&args[2..]));
    }

    tauri::Builder::default()
        .manage(AppState {
            armed: Mutex::new(false),
            arm_ctx: Mutex::new(None),
            pending: Mutex::new(None),
            hotkey: Mutex::new(settings::load(&exe_dir()).hotkey),
        })
        .plugin(
            tauri_plugin_global_shortcut::Builder::new()
                .with_handler(|app, shortcut, event| {
                    if event.state() != ShortcutState::Pressed {
                        return;
                    }
                    let arm_hotkey = {
                        let state: State<AppState> = app.state();
                        let raw = state.hotkey.lock().unwrap().clone();
                        parse_hotkey(&raw)
                    };
                    if *shortcut == arm_hotkey {
                        toggle_armed(app);
                    } else if shortcut.matches(Modifiers::empty(), Code::Escape) {
                        // Esc is contextual: an open tag entry is cancelled,
                        // otherwise the overlay disarms.
                        if has_pending(app) {
                            let state: State<AppState> = app.state();
                            let taken = state.pending.lock().unwrap().take();
                            if let Some(p) = taken {
                                let png = sweeps_dir().join(&p.sweep_name).join(&p.tag.image);
                                let _ = std::fs::remove_file(png);
                            }
                            let _ = app.emit("entry-cancelled", ());
                        } else {
                            apply_armed(app, false);
                        }
                    }
                })
                .build(),
        )
        .invoke_handler(tauri::generate_handler![
            set_armed,
            get_armed,
            capture_region,
            save_tag,
            cancel_tag,
            get_status,
            list_sweeps,
            load_sweep,
            update_tag,
            set_dropped,
            reorder_tags,
            export_sweep,
            get_settings,
            save_settings
        ])
        .setup(|app| {
            let handle = app.handle().clone();
            build_overlay(&handle)?;

            // Global hotkey from settings (default Ctrl+Shift+T) toggles
            // armed state.
            let hotkey_raw = settings::load(&exe_dir()).hotkey;
            app.global_shortcut().register(parse_hotkey(&hotkey_raw))?;

            // Tray icon and menu.
            let arm_item = MenuItem::with_id(app, "arm", "Arm (Ctrl+Shift+T)", true, None::<&str>)?;
            let review_item =
                MenuItem::with_id(app, "review", "Review and export", true, None::<&str>)?;
            let sweeps_item =
                MenuItem::with_id(app, "open-sweeps", "Open sweeps folder", true, None::<&str>)?;
            let settings_item =
                MenuItem::with_id(app, "settings", "Settings", true, None::<&str>)?;
            let quit_item = MenuItem::with_id(app, "quit", "Quit", true, None::<&str>)?;
            let menu = Menu::with_items(
                app,
                &[&arm_item, &review_item, &sweeps_item, &settings_item, &quit_item],
            )?;

            TrayIconBuilder::with_id("main")
                .icon(app.default_window_icon().unwrap().clone())
                .tooltip("TagFix")
                .menu(&menu)
                .show_menu_on_left_click(true)
                .on_menu_event(|app, event| match event.id().as_ref() {
                    "arm" => toggle_armed(app),
                    "review" => open_review(app),
                    "open-sweeps" => open_sweeps_folder(),
                    "settings" => open_settings(app),
                    "quit" => app.exit(0),
                    _ => {}
                })
                .build(app)?;

            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running TagFix");
}
