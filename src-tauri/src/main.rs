// TagFix: tag what is wrong on screen, get a fix list out.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::sync::Mutex;

use tagfix::capture;

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
    /// Bumped on every arm; the overlay JS echoes it back via
    /// overlay_ready so a dead webview is detectable.
    arm_epoch: Mutex<u64>,
    ready_epoch: Mutex<u64>,
}

fn now_utc() -> String {
    chrono::Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string()
}

/// Set once startup is fully finished; the launch guard exits the process
/// with an explanation if progress ever stalls before that.
static STARTUP_COMPLETE: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(false);

/// Last startup phase reached, with when it was reached.
static LAST_PHASE: std::sync::OnceLock<Mutex<(String, std::time::Instant)>> =
    std::sync::OnceLock::new();

/// Record a startup phase: timestamped line in tagfix-startup.log next to
/// the exe, plus the stall detector's reference point. The log is rewritten
/// on every run so it always describes the latest launch.
fn checkpoint(name: &str) {
    use std::io::Write;
    let lock = LAST_PHASE.get_or_init(|| {
        Mutex::new((String::new(), std::time::Instant::now()))
    });
    *lock.lock().unwrap() = (name.to_string(), std::time::Instant::now());
    if let Ok(mut f) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(exe_dir().join("tagfix-startup.log"))
    {
        let _ = writeln!(f, "{} {}", now_utc(), name);
    }
}

fn read_reg_value(key: &str, value: &str) -> String {
    std::process::Command::new("reg")
        .args(["query", key, "/v", value])
        .output()
        .ok()
        .map(|o| {
            String::from_utf8_lossy(&o.stdout)
                .lines()
                .filter(|l| l.contains(value))
                .collect::<Vec<_>>()
                .join(" ")
        })
        .unwrap_or_default()
        .trim()
        .to_string()
}

fn webview2_version() -> String {
    let machine = read_reg_value(
        r"HKLM\SOFTWARE\WOW6432Node\Microsoft\EdgeUpdate\Clients\{F3017226-FE2A-4295-8BDF-00C3A9A7E4C5}",
        "pv",
    );
    if !machine.is_empty() {
        return machine;
    }
    read_reg_value(
        r"HKCU\SOFTWARE\Microsoft\EdgeUpdate\Clients\{F3017226-FE2A-4295-8BDF-00C3A9A7E4C5}",
        "pv",
    )
}

/// WebView2 is the one thing the exe cannot carry inside itself. Warn
/// before window creation, because a broken runtime hangs, not errors.
fn preflight_webview2() {
    if webview2_version().is_empty() {
        message_box(
            "TagFix: WebView2 runtime not detected",
            "TagFix draws its overlay with Microsoft WebView2, which does not seem to be installed on this machine.\n\nInstall the WebView2 Evergreen runtime from:\nhttps://developer.microsoft.com/microsoft-edge/webview2\n\nTagFix will try to start anyway; if nothing appears it will exit with a message after 20 seconds.",
        );
    }
}

/// If any startup phase stalls (typically WebView2 refusing to come up),
/// exit with an explanation instead of sitting as a ghost window. Watches
/// until STARTUP_COMPLETE, so late phases are covered too.
fn launch_guard() {
    std::thread::spawn(|| loop {
        std::thread::sleep(std::time::Duration::from_secs(5));
        if STARTUP_COMPLETE.load(std::sync::atomic::Ordering::SeqCst) {
            return;
        }
        let (phase, since) = {
            let lock = LAST_PHASE.get_or_init(|| {
                Mutex::new((String::from("main"), std::time::Instant::now()))
            });
            let g = lock.lock().unwrap();
            (g.0.clone(), g.1.elapsed())
        };
        if since.as_secs() >= 25 {
            let msg = format!(
                "{} startup stalled at phase '{}' for {}s; webview2 detected: '{}'",
                now_utc(),
                phase,
                since.as_secs(),
                webview2_version()
            );
            let _ = std::fs::write(exe_dir().join("tagfix-error.log"), &msg);
            message_box(
                "TagFix could not start",
                &format!(
                    "TagFix stalled while starting (phase: {}) and shut itself down.\n\nThis usually means the WebView2 runtime is blocked or broken on this machine.\n\nSend tagfix-error.log and tagfix-startup.log from the folder next to tagfix.exe.",
                    phase
                ),
            );
            std::process::exit(1);
        }
    });
}

/// Native message box, used for fatal errors and watchdog notices so the
/// user is never left guessing at a silent failure.
fn message_box(title: &str, text: &str) {
    use windows::core::HSTRING;
    use windows::Win32::UI::WindowsAndMessaging::{MessageBoxW, MB_ICONWARNING, MB_OK};
    unsafe {
        MessageBoxW(
            None,
            &HSTRING::from(text),
            &HSTRING::from(title),
            MB_OK | MB_ICONWARNING,
        );
    }
}

/// Panics get written next to the exe and shown to the user instead of
/// vanishing (the release binary has no console).
fn install_panic_reporter() {
    std::panic::set_hook(Box::new(|info| {
        let msg = format!("{} {}", now_utc(), info);
        let _ = std::fs::write(exe_dir().join("tagfix-error.log"), &msg);
        message_box(
            "TagFix hit a fatal error",
            &format!(
                "TagFix could not continue.\n\n{}\n\nDetails were written to tagfix-error.log next to tagfix.exe. Run tagfix diag and send both files.",
                info
            ),
        );
    }));
}

/// Runtime trace log next to the exe, active in all builds. Arm and
/// capture write here so a hang in the field names its own step. Several
/// threads log at once, so writes are serialized to keep lines intact.
static RT_LOG_LOCK: Mutex<()> = Mutex::new(());

fn rt_log(msg: &str) {
    use std::io::Write;
    let _guard = RT_LOG_LOCK.lock();
    if let Ok(mut f) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(exe_dir().join("tagfix-runtime.log"))
    {
        let _ = writeln!(f, "{} {}", now_utc(), msg);
    }
}

/// Debug-build trace log next to the exe; a no-op in release builds.
fn dbg_log(msg: &str) {
    #[cfg(debug_assertions)]
    {
        use std::io::Write;
        if let Ok(mut f) = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(exe_dir().join("tagfix-debug.log"))
        {
            let _ = writeln!(f, "{} {}", now_utc(), msg);
        }
    }
    #[cfg(not(debug_assertions))]
    let _ = msg;
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
    rt_log("arm: reading cursor and monitors");
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
    rt_log("arm: reading foreground window");
    let fg = capture::foreground_info();

    rt_log("arm: positioning overlay on monitor");
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
    rt_log(&format!("apply_armed({}) enter", armed));
    let state: State<AppState> = app.state();
    *state.armed.lock().unwrap() = armed;

    if armed {
        let ctx = prepare_arm_context(app);
        *state.arm_ctx.lock().unwrap() = ctx;
    }

    if let Some(win) = app.get_webview_window("overlay") {
        // The overlay window only exists on screen while armed. Disarmed it
        // is fully hidden, so it can never block or obscure the desktop,
        // even on machines where window transparency fails.
        rt_log("apply_armed: setting cursor events");
        let _ = win.set_ignore_cursor_events(!armed);
        if armed {
            rt_log("apply_armed: showing overlay");
            let _ = win.show();
            rt_log("apply_armed: focusing overlay");
            let _ = win.set_focus();
        } else {
            let _ = win.hide();
        }
        rt_log("apply_armed: window ops done");
    }

    // Watchdog FIRST, before anything that could block: if arming wedges,
    // the overlay must still come off the screen. It hides the window
    // directly rather than routing back through this function.
    if armed {
        let epoch = {
            let state: State<AppState> = app.state();
            let mut e = state.arm_epoch.lock().unwrap();
            *e += 1;
            *e
        };
        let handle = app.clone();
        std::thread::spawn(move || {
            std::thread::sleep(std::time::Duration::from_secs(6));
            let state: State<AppState> = handle.state();
            let still_armed = *state.armed.lock().unwrap();
            let ready = *state.ready_epoch.lock().unwrap();
            drop(state);
            if still_armed && ready < epoch {
                rt_log("watchdog: overlay never reported ready, hiding overlay");
                if let Some(win) = handle.get_webview_window("overlay") {
                    let _ = win.set_ignore_cursor_events(true);
                    let _ = win.hide();
                }
                let st: State<AppState> = handle.state();
                *st.armed.lock().unwrap() = false;
                rt_log("watchdog: overlay hidden, disarmed");
                message_box(
                    "TagFix disarmed itself",
                    "The overlay did not draw within 6 seconds, so TagFix disarmed to keep the desktop usable.\n\nSend tagfix-runtime.log from the folder next to tagfix.exe.",
                );
            }
        });
    }

    // Esc must disarm even when another app holds keyboard focus, so it is
    // a global shortcut that only exists while armed. Registering it MUST
    // happen on a separate thread: apply_armed runs inside the global
    // shortcut handler, and calling register from that handler's own
    // thread deadlocks it (and with it the main thread, leaving the
    // overlay stuck on screen).
    {
        let handle = app.clone();
        std::thread::spawn(move || {
            let esc = Shortcut::new(None, Code::Escape);
            if armed {
                rt_log("esc: registering (off handler thread)");
                match handle.global_shortcut().register(esc) {
                    Ok(()) => rt_log("esc: registered"),
                    Err(e) => rt_log(&format!(
                        "esc: register failed ({}); Esc still disarms while the overlay has focus",
                        e
                    )),
                }
            } else {
                rt_log("esc: unregistering (off handler thread)");
                match handle.global_shortcut().unregister(esc) {
                    Ok(()) => rt_log("esc: unregistered"),
                    Err(e) => rt_log(&format!("esc: unregister failed ({})", e)),
                }
            }
        });
    }

    // Emitting reaches into the webview; keep it off the critical path so
    // a wedged renderer cannot block arming either.
    {
        let handle = app.clone();
        std::thread::spawn(move || {
            rt_log("emit: armed-changed");
            let _ = handle.emit("armed-changed", armed);
            rt_log("emit: done");
        });
    }
    rt_log("apply_armed: returning");
}

/// The overlay JS calls this as soon as the armed UI is on screen.
#[tauri::command]
fn overlay_ready(state: State<AppState>) {
    rt_log("overlay_ready received from webview");
    let epoch = *state.arm_epoch.lock().unwrap();
    *state.ready_epoch.lock().unwrap() = epoch;
}

/// The overlay JS calls this once its script has booted. If this line
/// never appears in the runtime log, the webview content never loaded.
#[tauri::command]
fn ui_loaded() {
    rt_log("overlay page loaded (JS booted)");
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

    rt_log(&format!(
        "capture: starting for region {},{} {}x{} on {}",
        px, py, pw, ph, ctx.monitor_name
    ));
    let monitor_name = ctx.monitor_name.clone();
    let capture_path = out_path.clone();
    let screen_x = ctx.monitor_x + px as i32;
    let screen_y = ctx.monitor_y + py as i32;
    let method = tauri::async_runtime::spawn_blocking(move || {
        capture::capture_region_with_fallback(
            &monitor_name,
            region,
            screen_x,
            screen_y,
            capture_path,
        )
    })
    .await
    .map_err(|e| e.to_string())?
    .map_err(|e| {
        rt_log(&format!("capture: FAILED: {}", e));
        e
    })?;
    rt_log(&format!("capture: done via {}", method));

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

fn open_help(app: &AppHandle) {
    if let Some(w) = app.get_webview_window("help") {
        let _ = w.show();
        let _ = w.set_focus();
        return;
    }
    let _ = WebviewWindowBuilder::new(app, "help", WebviewUrl::App("help.html".into()))
        .title("How to use TagFix")
        .inner_size(560.0, 720.0)
        .build();
}

fn open_sweeps_folder() {
    let dir = sweeps_dir();
    let _ = std::fs::create_dir_all(&dir);
    let _ = std::process::Command::new("explorer.exe").arg(&dir).spawn();
}

fn build_overlay(app: &AppHandle) -> tauri::Result<()> {
    checkpoint("overlay webview creating");
    let win = WebviewWindowBuilder::new(app, "overlay", WebviewUrl::App("index.html".into()))
        .title("TagFix overlay")
        .transparent(true)
        .decorations(false)
        .always_on_top(true)
        .skip_taskbar(true)
        .shadow(false)
        .visible(false)
        .focused(false)
        .build()?;

    if let Ok(Some(monitor)) = win.primary_monitor() {
        let _ = win.set_position(monitor.position().clone());
        let _ = win.set_size(monitor.size().clone());
    }
    // Start disarmed: click-through.
    let _ = win.set_ignore_cursor_events(true);
    checkpoint("overlay webview created");
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

/// `tagfix diag`: environment report for debugging misbehaving machines.
/// Prints to the console and writes tagfix-diag.txt next to the exe.
fn run_diag() -> i32 {
    let mut out = String::new();
    out.push_str(&format!("TagFix diag {} at {}\n", env!("CARGO_PKG_VERSION"), now_utc()));
    out.push_str(&format!("exe: {}\n", std::env::current_exe().map(|p| p.display().to_string()).unwrap_or_default()));

    out.push_str(&format!(
        "os: {} / {}\n",
        read_reg_value(r"HKLM\SOFTWARE\Microsoft\Windows NT\CurrentVersion", "ProductName"),
        read_reg_value(r"HKLM\SOFTWARE\Microsoft\Windows NT\CurrentVersion", "CurrentBuild")
    ));

    let wv = webview2_version();
    out.push_str(&format!(
        "webview2 runtime: {}\n",
        if wv.is_empty() { "NOT FOUND (install the WebView2 Evergreen runtime)".into() } else { wv }
    ));

    unsafe {
        let dwm = windows::Win32::Graphics::Dwm::DwmIsCompositionEnabled();
        out.push_str(&format!("dwm composition: {:?}\n", dwm));
    }

    match windows_capture::monitor::Monitor::enumerate() {
        Ok(mons) => {
            out.push_str(&format!("monitors: {}\n", mons.len()));
            for m in mons {
                out.push_str(&format!(
                    "  index {:?} device {:?} size {:?}x{:?} refresh {:?}\n",
                    m.index().ok(),
                    m.device_name().ok(),
                    m.width().ok(),
                    m.height().ok(),
                    m.refresh_rate().ok()
                ));
            }
        }
        Err(e) => out.push_str(&format!("monitors: ENUMERATION FAILED: {}\n", e)),
    }

    unsafe {
        use windows::Win32::UI::Input::KeyboardAndMouse::{
            RegisterHotKey, UnregisterHotKey, HOT_KEY_MODIFIERS, MOD_CONTROL, MOD_SHIFT,
        };
        let probes: [(&str, HOT_KEY_MODIFIERS, u32); 3] = [
            ("ctrl+shift+t", MOD_CONTROL | MOD_SHIFT, 0x54),
            ("ctrl+shift+r", MOD_CONTROL | MOD_SHIFT, 0x52),
            ("esc", HOT_KEY_MODIFIERS(0), 0x1B),
        ];
        let tagfix_running = {
            let me = std::process::id();
            std::process::Command::new("tasklist")
                .args(["/FI", "IMAGENAME eq tagfix.exe", "/FO", "CSV", "/NH"])
                .output()
                .ok()
                .map(|o| {
                    String::from_utf8_lossy(&o.stdout)
                        .lines()
                        .filter(|l| l.contains("tagfix.exe") && !l.contains(&format!("\"{}\"", me)))
                        .count()
                })
                .unwrap_or(0)
        };
        out.push_str(&format!("other tagfix instances running: {}\n", tagfix_running));
        for (name, mods, vk) in probes {
            let free = RegisterHotKey(None, 990 + vk as i32, mods, vk).is_ok();
            if free {
                let _ = UnregisterHotKey(None, 990 + vk as i32);
            }
            out.push_str(&format!(
                "hotkey {}: {}{}\n",
                name,
                if free { "free" } else { "TAKEN by another app" },
                if !free && tagfix_running > 0 {
                    " (a running TagFix owns its own hotkeys; close it and rerun diag to test)"
                } else {
                    ""
                }
            ));
        }
    }

    println!("{}", out);
    let path = exe_dir().join("tagfix-diag.txt");
    match std::fs::write(&path, &out) {
        Ok(()) => println!("written to {}", path.display()),
        Err(e) => eprintln!("could not write {}: {}", path.display(), e),
    }
    0
}

fn main() {
    install_panic_reporter();
    let args: Vec<String> = std::env::args().collect();
    if args.len() >= 2 && args[1] == "diag" {
        #[cfg(not(debug_assertions))]
        unsafe {
            use windows::Win32::System::Console::{AttachConsole, ATTACH_PARENT_PROCESS};
            let _ = AttachConsole(ATTACH_PARENT_PROCESS);
        }
        std::process::exit(run_diag());
    }
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

    let _ = std::fs::remove_file(exe_dir().join("tagfix-startup.log"));
    checkpoint("main start");
    preflight_webview2();
    launch_guard();
    checkpoint("building tauri app");

    tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|_app, _args, _cwd| {
            // A second launch lands here in the first instance's process.
            std::thread::spawn(|| {
                message_box(
                    "TagFix is already running",
                    "TagFix is already running in the tray.\n\nCtrl+Shift+T arms it, Ctrl+Shift+R opens review and export.",
                );
            });
        }))
        .manage(AppState {
            armed: Mutex::new(false),
            arm_ctx: Mutex::new(None),
            pending: Mutex::new(None),
            hotkey: Mutex::new(settings::load(&exe_dir()).hotkey),
            arm_epoch: Mutex::new(0),
            ready_epoch: Mutex::new(0),
        })
        .plugin(
            tauri_plugin_global_shortcut::Builder::new()
                .with_handler(|app, shortcut, event| {
                    dbg_log(&format!("shortcut event: {:?} state {:?}", shortcut, event.state()));
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
                    } else if shortcut.matches(Modifiers::CONTROL | Modifiers::SHIFT, Code::KeyR) {
                        open_review(app);
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
            save_settings,
            overlay_ready,
            ui_loaded
        ])
        .setup(|app| {
            checkpoint("setup entered");
            let handle = app.handle().clone();
            build_overlay(&handle)?;

            // Global hotkeys. Registration failure means some other program
            // owns the combination; that must never kill startup, only warn.
            let mut hotkey_problems: Vec<String> = Vec::new();
            let hotkey_raw = settings::load(&exe_dir()).hotkey;
            if app
                .global_shortcut()
                .register(parse_hotkey(&hotkey_raw))
                .is_err()
            {
                hotkey_problems.push(format!(
                    "The arm hotkey ({}) is taken by another program, so arming from the keyboard will not work. Set a different hotkey in Settings (tray icon, Settings).",
                    hotkey_raw
                ));
            }
            // Ctrl+Shift+R opens review and export; the tool is keyboard
            // first and some shells hide fresh tray icons.
            if app
                .global_shortcut()
                .register(Shortcut::new(
                    Some(Modifiers::CONTROL | Modifiers::SHIFT),
                    Code::KeyR,
                ))
                .is_err()
            {
                hotkey_problems.push(
                    "The review hotkey (ctrl+shift+r) is taken by another program. Use the tray menu, Review and export.".to_string(),
                );
            }
            if !hotkey_problems.is_empty() {
                let text = format!(
                    "TagFix started, but:\n\n{}",
                    hotkey_problems.join("\n\n")
                );
                // Own thread: a modal box must not stall setup.
                std::thread::spawn(move || message_box("TagFix hotkey conflict", &text));
            }
            checkpoint("hotkeys registered");

            // Tray icon and menu.
            let arm_item = MenuItem::with_id(app, "arm", "Arm (Ctrl+Shift+T)", true, None::<&str>)?;
            let review_item = MenuItem::with_id(
                app,
                "review",
                "Review and export (Ctrl+Shift+R)",
                true,
                None::<&str>,
            )?;
            let sweeps_item =
                MenuItem::with_id(app, "open-sweeps", "Open sweeps folder", true, None::<&str>)?;
            let settings_item =
                MenuItem::with_id(app, "settings", "Settings", true, None::<&str>)?;
            let help_item =
                MenuItem::with_id(app, "help", "How to use", true, None::<&str>)?;
            let quit_item = MenuItem::with_id(app, "quit", "Quit", true, None::<&str>)?;
            let menu = Menu::with_items(
                app,
                &[&arm_item, &review_item, &sweeps_item, &settings_item, &help_item, &quit_item],
            )?;

            checkpoint("tray building");
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
                    "help" => open_help(app),
                    "quit" => app.exit(0),
                    _ => {}
                })
                .build(app)?;
            checkpoint("tray built");

            // First launch: a NATIVE summary box, deliberately not a
            // webview, so machines where webview creation misbehaves still
            // get told how the tool works. The rich guide stays in the
            // tray menu (How to use).
            let mut startup_settings = settings::load(&exe_dir());
            if !startup_settings.help_shown {
                startup_settings.help_shown = true;
                let _ = settings::save(&exe_dir(), &startup_settings);
                std::thread::spawn(|| {
                    message_box(
                        "Welcome to TagFix",
                        "Tag what is wrong on screen, get a fix list out.\n\nCtrl+Shift+T arms the overlay: drag a box around a problem, type what is wrong, press Enter, tag the next thing. Esc cancels or disarms.\n\nCtrl+Shift+R opens review and export.\n\nFull guide: tray icon, How to use.",
                    );
                });
            }

            STARTUP_COMPLETE.store(true, std::sync::atomic::Ordering::SeqCst);
            checkpoint("startup complete");

            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running TagFix");
}
