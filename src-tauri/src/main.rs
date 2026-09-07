// TagFix: tag what is wrong on screen, get a fix list out.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod hook;

use std::sync::Mutex;

use tagfix::capture;

use serde::Serialize;
use tauri::menu::{Menu, MenuItem};
use tauri::tray::TrayIconBuilder;
use tauri::{AppHandle, Emitter, Manager, State, WebviewUrl, WebviewWindowBuilder};
use tauri_plugin_global_shortcut::{Code, GlobalShortcutExt, Modifiers, Shortcut, ShortcutState};

use tagfix::settings::{self, Settings};
use tagfix::store::{self, Attachment, SweepStore, Tag};
use tagfix::{context, quote};

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

/// Where the next crop goes when it is an attachment rather than a new
/// tag: Ctrl+Shift+A (a comparison) or the review window's Capture after.
/// `sweep_name` is None when the caller means "whichever sweep is active".
#[derive(Clone)]
struct AttachTarget {
    sweep_name: Option<String>,
    number: u32,
    label: String,
}

/// The single recall slot behind Ctrl+Up in the popover.
#[derive(Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
struct LastNote {
    text: String,
    severity: String,
    area: String,
    target: String,
}

struct AppState {
    armed: Mutex<bool>,
    pending: Mutex<Option<PendingTag>>,
    hotkey: Mutex<String>,
    quote_hotkey: Mutex<String>,
    attach_hotkey: Mutex<String>,
    /// The last tag saved in this armed session, which is what Ctrl+Shift+A
    /// attaches to. Cleared on disarm: attaching to yesterday's tag by
    /// accident would be worse than saying there is nothing to attach to.
    last_saved: Mutex<Option<(String, u32)>>,
    attach: Mutex<Option<AttachTarget>>,
    last_note: Mutex<Option<LastNote>>,
    /// Where the last highlight ended, so the quote hotkey can look up the
    /// element the operator was reading.
    last_highlight: Mutex<Option<(i32, i32)>>,
    /// Origin and scale of the monitor the pen chip was placed on, so the
    /// CSS rectangle the overlay reports converts back to screen pixels.
    chip_monitor: Mutex<Option<((i32, i32), f64)>>,
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

/// The same, for a hotkey whose default is not the arm one.
fn parse_hotkey_or(raw: &str, fallback: &str) -> Shortcut {
    raw.parse::<Shortcut>()
        .or_else(|_| fallback.parse::<Shortcut>())
        .unwrap_or_else(|_| Shortcut::new(Some(Modifiers::CONTROL | Modifiers::SHIFT), Code::KeyT))
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

/// Describe the monitor holding a screen point, and the app in front at
/// that moment. Called when a selection starts, while the overlay is
/// still click-through, so the foreground window is the app under test.
fn context_for_point(app: &AppHandle, x: i32, y: i32) -> Option<ArmContext> {
    let win = app.get_webview_window("overlay")?;
    let monitors = win.available_monitors().ok()?;

    let mut chosen = None;
    for (i, m) in monitors.iter().enumerate() {
        let p = m.position();
        let s = m.size();
        let inside_x = x >= p.x && x < p.x + s.width as i32;
        let inside_y = y >= p.y && y < p.y + s.height as i32;
        if inside_x && inside_y {
            chosen = Some((i, m.clone()));
            break;
        }
    }
    let (index, monitor) = match chosen {
        Some(v) => v,
        None => (0, win.primary_monitor().ok()??),
    };

    // The overlay is click-through in standby, so whatever is in front is
    // the operator's actual app.
    let fg = capture::foreground_info();

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

/// Put the overlay on the monitor that holds a selection, without taking
/// focus or interactivity away from the operator's app.
fn place_overlay_for(app: &AppHandle, ctx: &ArmContext) {
    if let Some(win) = app.get_webview_window("overlay") {
        let _ = win.set_position(tauri::PhysicalPosition::new(ctx.monitor_x, ctx.monitor_y));
        let _ = win.set_size(tauri::PhysicalSize::new(ctx.monitor_w, ctx.monitor_h));
    }
}

/// Standby keeps the overlay visible but click-through so the machine
/// stays usable; entry mode makes it interactive to take typed text.
fn set_overlay_interactive(app: &AppHandle, interactive: bool) {
    if let Some(win) = app.get_webview_window("overlay") {
        let _ = win.set_ignore_cursor_events(!interactive);
        if interactive {
            let _ = win.set_focus();
        }
    }
    // The capture gesture is off while a tag is being typed, and comes
    // back only if the app is still armed.
    if interactive {
        hook::set_armed(false);
    } else {
        let state: State<AppState> = app.state();
        let armed = *state.armed.lock().unwrap();
        hook::set_armed(armed);
    }
    rt_log(&format!("overlay interactive: {}", interactive));
}

fn apply_armed(app: &AppHandle, armed: bool) {
    rt_log(&format!("apply_armed({}) enter", armed));
    let state: State<AppState> = app.state();
    *state.armed.lock().unwrap() = armed;

    // Standby: the overlay is on screen but click-through, so the operator
    // keeps using the machine normally. Nothing here takes focus and no
    // global key is swallowed; the capture gesture arrives through the
    // mouse hook instead.
    if let Some(win) = app.get_webview_window("overlay") {
        rt_log("apply_armed: setting click-through");
        let _ = win.set_ignore_cursor_events(true);
        if armed {
            rt_log("apply_armed: showing overlay");
            let _ = win.show();
        } else {
            let _ = win.hide();
        }
        rt_log("apply_armed: window ops done");
    }

    // Discard any half finished selection or unsaved tag when disarming.
    if !armed {
        let taken = {
            let st: State<AppState> = app.state();
            // An armed session owns its attach target and its "last tag":
            // both are about what just happened on screen.
            *st.last_saved.lock().unwrap() = None;
            *st.attach.lock().unwrap() = None;
            *st.last_highlight.lock().unwrap() = None;
            let mut g = st.pending.lock().unwrap();
            g.take()
        };
        if let Some(p) = taken {
            if let Some(image) = &p.tag.image {
                let png = sweeps_dir().join(&p.sweep_name).join(image);
                let _ = std::fs::remove_file(png);
            }
        }
    }

    hook::set_armed(armed);

    // Emitting reaches into the webview; keep it off the critical path so
    // a wedged renderer cannot block arming.
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

/// Say something brief on the overlay. The overlay owns how long it stays;
/// duration 0 means "until something replaces it".
fn toast(app: &AppHandle, message: &str, duration: u32) {
    let _ = app.emit(
        "toast",
        serde_json::json!({ "message": message, "duration": duration }),
    );
}

/// Where the pointer is right now, for the quote hotkey when no highlight
/// gesture came through the hook first.
fn cursor_point() -> Option<(i32, i32)> {
    use windows::Win32::Foundation::POINT;
    use windows::Win32::UI::WindowsAndMessaging::GetCursorPos;
    unsafe {
        let mut p = POINT::default();
        GetCursorPos(&mut p).ok()?;
        Some((p.x, p.y))
    }
}

/// The sweep a new tag belongs to, honouring the day rollover setting: with
/// it on, the first capture of a day starts <date>-default rather than
/// extending yesterday's sweep.
fn open_active_sweep(store: &SweepStore, ts: &str) -> std::io::Result<(String, store::Sweep)> {
    if settings::load(&exe_dir()).new_sweep_each_day && ts.len() >= 10 {
        let date = &ts[..10];
        if !store.sweep_exists_for_date(date)? {
            rt_log(&format!("sweep: day rollover, starting {}-default", date));
            return store.create_sweep("default", ts);
        }
    }
    store.active_sweep(ts)
}

/// The element under a screen point. Our own overlay covers the monitor
/// while armed, so a hit on this process means blinking it off for a frame
/// and asking once more; anything else is not worth a second capture's
/// delay.
fn element_with_retry(app: &AppHandle, x: i32, y: i32) -> String {
    match context::element_at_point(x, y) {
        context::ElementHit::Found(name) => name,
        context::ElementHit::None => String::new(),
        context::ElementHit::OwnProcess => {
            rt_log("element: hit our own overlay, retrying with it hidden");
            let win = app.get_webview_window("overlay");
            if let Some(w) = &win {
                let _ = w.hide();
            }
            std::thread::sleep(std::time::Duration::from_millis(70));
            let again = context::element_at_point(x, y);
            if let Some(w) = &win {
                let armed = {
                    let state: State<AppState> = app.state();
                    let v = *state.armed.lock().unwrap();
                    v
                };
                if armed {
                    let _ = w.set_ignore_cursor_events(true);
                    let _ = w.show();
                }
            }
            match again {
                context::ElementHit::Found(name) => name,
                _ => String::new(),
            }
        }
    }
}

/// The target chip values the popover offers, in settings order. "other"
/// is added by the overlay and always comes last.
fn target_names(settings: &Settings) -> Vec<String> {
    settings
        .targets
        .iter()
        .map(|t| t.name.clone())
        .filter(|n| !n.trim().is_empty())
        .collect()
}

/// The context frame beside a region crop. Every failure logs and returns
/// None: a missing frame is a smaller loss than a lost crop.
fn write_context_frame_for(
    dir: &std::path::Path,
    number: u32,
    crop: context::ScreenRect,
) -> Option<String> {
    let (device, origin) = context::foreground_monitor()?;
    let frame = context::foreground_frame_bounds()?;
    let name = store::tag_context_image_name(number);
    match context::write_context_frame(&device, origin, frame, crop, &dir.join(&name)) {
        Ok(()) => Some(name),
        Err(e) => {
            rt_log(&format!("context frame: skipped ({})", e));
            None
        }
    }
}

/// Capture the whole foreground window beside a quote tag. Optional, off by
/// default, and never fatal.
fn capture_quote_window(dir: &std::path::Path, number: u32) -> Option<String> {
    let (device, origin) = context::foreground_monitor()?;
    let frame = context::foreground_frame_bounds()?;
    let name = store::tag_quote_image_name(number);
    let region = capture::MonitorRegion {
        x: (frame.x - origin.0).max(0) as u32,
        y: (frame.y - origin.1).max(0) as u32,
        width: frame.width,
        height: frame.height,
    };
    match capture::capture_region_with_fallback(&device, region, frame.x, frame.y, dir.join(&name)) {
        Ok(method) => {
            rt_log(&format!("quote screenshot: written via {}", method));
            Some(name)
        }
        Err(e) => {
            rt_log(&format!("quote screenshot: skipped ({})", e));
            None
        }
    }
}

/// One quote tag from the highlight the operator has already made.
///
/// Must not run on the main thread: grabbing the selection injects keys and
/// waits up to 400 ms for the clipboard, and the mouse hook lives on the
/// main thread.
fn start_quote(app: &AppHandle, point: Option<(i32, i32)>) {
    let armed = {
        let state: State<AppState> = app.state();
        let v = *state.armed.lock().unwrap();
        v
    };
    if !armed {
        return;
    }
    let settings = settings::load(&exe_dir());
    // The chip has done its job either way.
    hook::clear_chip_rect();
    let _ = app.emit("chip-dismiss", ());

    rt_log("quote: grabbing the highlight");
    let grabbed = match quote::grab_selection() {
        Ok(g) => g,
        Err(reason) => {
            rt_log(&format!("quote: {}", reason));
            let _ = app.emit("selection-cancel", serde_json::json!({ "reason": reason }));
            return;
        }
    };

    let point = point.or_else(cursor_point).unwrap_or((0, 0));
    let ctx = match context_for_point(app, point.0, point.1) {
        Some(c) => c,
        None => {
            let _ = app.emit(
                "selection-cancel",
                serde_json::json!({ "reason": "could not read the monitor layout" }),
            );
            return;
        }
    };

    let ts = now_utc();
    let store = SweepStore::new(sweeps_dir());
    let (sweep_name, sweep) = match open_active_sweep(&store, &ts) {
        Ok(v) => v,
        Err(e) => {
            rt_log(&format!("quote: sweep open failed: {}", e));
            let _ = app.emit(
                "selection-cancel",
                serde_json::json!({ "reason": "could not open the sweep folder" }),
            );
            return;
        }
    };
    let number = sweep.next_tag_number();
    let dir = store.root().join(&sweep_name);

    // Optional: the window the quote came from, as pixels. Our own chrome
    // goes off screen first, exactly like a region capture.
    let image = if settings.quote_screenshot {
        let _ = app.emit("selection-hide", ());
        std::thread::sleep(std::time::Duration::from_millis(110));
        capture_quote_window(&dir, number)
    } else {
        None
    };

    let url = context::foreground_url();
    let element = element_with_retry(app, point.0, point.1);
    let target = settings::target_for_url(&settings.targets, &url)
        .unwrap_or_default()
        .to_string();

    let tag = Tag {
        number,
        image,
        captured_utc: ts,
        monitor_index: ctx.monitor_index,
        dpi_scale: ctx.dpi_scale,
        region: None,
        window_title: ctx.window_title.clone(),
        process_name: ctx.process_name.clone(),
        screen_resolution: format!("{}x{}", ctx.monitor_w, ctx.monitor_h),
        kind: store::KIND_QUOTE.to_string(),
        quote: grabbed.text.clone(),
        quote_html: grabbed.html.clone(),
        url,
        element,
        target: target.clone(),
        ..Tag::default()
    };
    {
        let st: State<AppState> = app.state();
        *st.pending.lock().unwrap() = Some(PendingTag {
            sweep_name: sweep_name.clone(),
            tag,
        });
    }

    let scale = if ctx.dpi_scale <= 0.0 { 1.0 } else { ctx.dpi_scale };
    let css = serde_json::json!({
        "x": (point.0 - ctx.monitor_x) as f64 / scale,
        "y": (point.1 - ctx.monitor_y) as f64 / scale,
        "w": 0.0,
        "h": 0.0,
        "kind": store::KIND_QUOTE,
        "quote": grabbed.text,
        "tagNumber": number,
        "sweepName": sweep_name,
        "targets": target_names(&settings),
        "target": target,
    });
    place_overlay_for(app, &ctx);
    set_overlay_interactive(app, true);
    let _ = app.emit("entry-open", css);
    rt_log(&format!("quote: entry open for tag {}", number));
}

/// The crop that has just been taken belongs to an existing tag, not to a
/// new one. No popover: an attachment is evidence, not a note.
fn finish_attachment(
    app: &AppHandle,
    ctx: &ArmContext,
    selection: &capture::Selection,
    attach: AttachTarget,
) {
    let ts = now_utc();
    let store = SweepStore::new(sweeps_dir());
    let sweep_name = match attach.sweep_name.clone() {
        Some(name) => name,
        None => match open_active_sweep(&store, &ts) {
            Ok((name, _)) => name,
            Err(e) => {
                rt_log(&format!("attach: sweep open failed: {}", e));
                toast(app, "could not open the sweep folder", 3000);
                return;
            }
        },
    };
    let sweep = match store.load_sweep(&sweep_name) {
        Ok(s) => s,
        Err(e) => {
            rt_log(&format!("attach: sweep load failed: {}", e));
            toast(app, "could not open the sweep folder", 3000);
            return;
        }
    };
    let Some(tag) = sweep.tags.iter().find(|t| t.number == attach.number) else {
        toast(
            app,
            &format!("tag {} is not in this sweep", attach.number),
            3000,
        );
        return;
    };
    let image = tag.next_attachment_name();
    let out_path = store.root().join(&sweep_name).join(&image);

    let _ = app.emit("selection-hide", ());
    std::thread::sleep(std::time::Duration::from_millis(110));

    let region = selection.region_on((ctx.monitor_x, ctx.monitor_y));
    if let Err(e) = capture::capture_region_with_fallback(
        &ctx.monitor_name,
        region,
        selection.screen_x,
        selection.screen_y,
        out_path,
    ) {
        rt_log(&format!("attach: capture FAILED: {}", e));
        let _ = app.emit(
            "selection-cancel",
            serde_json::json!({ "reason": "capture failed, see tagfix-runtime.log" }),
        );
        return;
    }

    let attachment = Attachment {
        image,
        region: store::Rect {
            x: selection.screen_x,
            y: selection.screen_y,
            width: selection.width,
            height: selection.height,
        },
        captured_utc: ts,
        label: attach.label.clone(),
    };
    match store.append_attachment(&sweep_name, attach.number, attachment) {
        Ok(_) => {
            rt_log(&format!(
                "attach: {} crop added to tag {}",
                attach.label, attach.number
            ));
            toast(app, &format!("attached to tag {}", attach.number), 1800);
        }
        Err(e) => {
            rt_log(&format!("attach: save failed: {}", e));
            toast(app, "could not save the attachment", 3000);
        }
    }
}

/// Drive one selection through to a captured, pending tag. Runs on the
/// hook worker thread, never on the hook callback itself.
fn handle_selection_end(app: &AppHandle, ctx: ArmContext, start: (i32, i32), end: (i32, i32)) {
    let selection = capture::Selection::from_drag(start, end);
    if selection.too_small() {
        rt_log(&format!(
            "selection: too small ({}x{}), ignored",
            selection.width, selection.height
        ));
        let _ = app.emit(
            "selection-cancel",
            serde_json::json!({ "reason": "selection too small, drag a larger box" }),
        );
        return;
    }
    // Armed by Ctrl+Shift+A or by the review window: this crop belongs to a
    // tag that already exists.
    let attach = {
        let st: State<AppState> = app.state();
        let mut g = st.attach.lock().unwrap();
        g.take()
    };
    if let Some(a) = attach {
        finish_attachment(app, &ctx, &selection, a);
        return;
    }

    let origin = (ctx.monitor_x, ctx.monitor_y);
    let region = selection.region_on(origin);
    let (x0, y0) = (selection.screen_x, selection.screen_y);
    let (w, h) = (selection.width, selection.height);

    let ts = now_utc();
    let store = SweepStore::new(sweeps_dir());
    let (sweep_name, sweep) = match open_active_sweep(&store, &ts) {
        Ok(v) => v,
        Err(e) => {
            rt_log(&format!("selection: sweep open failed: {}", e));
            let _ = app.emit(
                "selection-cancel",
                serde_json::json!({ "reason": "could not open the sweep folder" }),
            );
            return;
        }
    };
    let number = sweep.next_tag_number();
    let image = store::tag_image_name(number);
    let out_path = store.root().join(&sweep_name).join(&image);

    // Take our own chrome off screen before grabbing pixels.
    let _ = app.emit("selection-hide", ());
    std::thread::sleep(std::time::Duration::from_millis(110));

    rt_log(&format!(
        "capture: starting for region {},{} {}x{} on {}",
        region.x, region.y, region.width, region.height, ctx.monitor_name
    ));
    let method = match capture::capture_region_with_fallback(
        &ctx.monitor_name,
        region,
        x0,
        y0,
        out_path,
    ) {
        Ok(m) => m,
        Err(e) => {
            rt_log(&format!("capture: FAILED: {}", e));
            let _ = app.emit(
                "selection-cancel",
                serde_json::json!({ "reason": "capture failed, see tagfix-runtime.log" }),
            );
            return;
        }
    };
    rt_log(&format!("capture: done via {}", method));

    // Context, in the order that costs the crop least: the pixels are
    // already on disk, so a slow browser or a missing frame only costs
    // metadata. Each lookup has its own quarter second budget.
    let settings = settings::load(&exe_dir());
    let url = context::foreground_url();
    let element = element_with_retry(app, start.0, start.1);
    let target = settings::target_for_url(&settings.targets, &url)
        .unwrap_or_default()
        .to_string();
    let context_image = if settings.context_frame {
        write_context_frame_for(
            &store.root().join(&sweep_name),
            number,
            context::ScreenRect {
                x: x0,
                y: y0,
                width: w,
                height: h,
            },
        )
    } else {
        None
    };

    let tag = Tag {
        number,
        image: Some(image),
        captured_utc: ts,
        monitor_index: ctx.monitor_index,
        dpi_scale: ctx.dpi_scale,
        region: Some(store::Rect {
            x: x0,
            y: y0,
            width: w,
            height: h,
        }),
        window_title: ctx.window_title.clone(),
        process_name: ctx.process_name.clone(),
        screen_resolution: format!("{}x{}", ctx.monitor_w, ctx.monitor_h),
        text: String::new(),
        severity: String::new(),
        area: String::new(),
        dropped: false,
        context_image,
        url,
        element,
        target: target.clone(),
        ..Tag::default()
    };
    {
        let st: State<AppState> = app.state();
        *st.pending.lock().unwrap() = Some(PendingTag {
            sweep_name: sweep_name.clone(),
            tag,
        });
    }

    // Entry mode: the overlay takes clicks and keys just long enough for
    // the operator to describe the tag.
    let r = selection.css_rect(origin, ctx.dpi_scale);
    let css = serde_json::json!({
        "x": r.x,
        "y": r.y,
        "w": r.w,
        "h": r.h,
        "kind": store::KIND_REGION,
        "quote": "",
        "tagNumber": number,
        "sweepName": sweep_name,
        "targets": target_names(&settings),
        "target": target,
    });
    set_overlay_interactive(app, true);
    let _ = app.emit("entry-open", css);
    rt_log("entry: open");
}

/// Worker that turns hook events into overlay updates and captures.
fn spawn_hook_worker(app: AppHandle, rx: std::sync::mpsc::Receiver<hook::HookEvent>) {
    std::thread::spawn(move || {
        let mut ctx: Option<ArmContext> = None;
        let mut start = (0, 0);
        let mut last_emit = std::time::Instant::now();
        let mut last_ignored = std::time::Instant::now()
            .checked_sub(std::time::Duration::from_secs(10))
            .unwrap_or_else(std::time::Instant::now);
        for event in rx {
            match event {
                hook::HookEvent::Start(x, y) => {
                    rt_log(&format!("selection: start at {},{}", x, y));
                    start = (x, y);
                    ctx = context_for_point(&app, x, y);
                    if let Some(c) = ctx.as_ref() {
                        place_overlay_for(&app, c);
                        let _ = app.emit("selection-start", ());
                    }
                }
                hook::HookEvent::Update(x, y) => {
                    let Some(c) = ctx.as_ref() else { continue };
                    // The hook fires per mouse move; keep the UI updates
                    // to roughly one frame.
                    if last_emit.elapsed() < std::time::Duration::from_millis(16) {
                        continue;
                    }
                    last_emit = std::time::Instant::now();
                    let r = capture::Selection::from_drag(start, (x, y))
                        .css_rect((c.monitor_x, c.monitor_y), c.dpi_scale);
                    let rect = serde_json::json!({
                        "x": r.x, "y": r.y, "w": r.w, "h": r.h,
                    });
                    let _ = app.emit("selection-update", rect);
                }
                hook::HookEvent::End(x, y) => {
                    rt_log(&format!("selection: end at {},{}", x, y));
                    let _ = app.emit("one-shot", false);
                    if let Some(c) = ctx.take() {
                        handle_selection_end(&app, c, start, (x, y));
                    }
                }
                hook::HookEvent::Highlight(x, y) => {
                    {
                        let st: State<AppState> = app.state();
                        *st.last_highlight.lock().unwrap() = Some((x, y));
                    }
                    // The chip is an offer, not an interruption: it is
                    // suppressed while a tag is being typed, and can be
                    // turned off entirely.
                    let busy = {
                        let st: State<AppState> = app.state();
                        let v = st.pending.lock().unwrap().is_some();
                        v
                    };
                    if busy || !settings::load(&exe_dir()).show_pen_chip {
                        continue;
                    }
                    let Some(c) = context_for_point(&app, x, y) else {
                        continue;
                    };
                    let scale = if c.dpi_scale <= 0.0 { 1.0 } else { c.dpi_scale };
                    {
                        let st: State<AppState> = app.state();
                        *st.chip_monitor.lock().unwrap() =
                            Some(((c.monitor_x, c.monitor_y), scale));
                    }
                    place_overlay_for(&app, &c);
                    let _ = app.emit(
                        "highlight-hint",
                        serde_json::json!({
                            "x": (x - c.monitor_x) as f64 / scale,
                            "y": (y - c.monitor_y) as f64 / scale,
                        }),
                    );
                }
                hook::HookEvent::QuoteClick => {
                    rt_log("chip: clicked");
                    let point = {
                        let st: State<AppState> = app.state();
                        let v = *st.last_highlight.lock().unwrap();
                        v
                    };
                    start_quote(&app, point);
                }
                hook::HookEvent::ChipDismiss => {
                    hook::clear_chip_rect();
                    let _ = app.emit("chip-dismiss", ());
                }
                hook::HookEvent::Ignored { ctrl, shift } => {
                    // Rate limited: one line per second is enough to tell
                    // whether clicks reach the hook and what the keyboard
                    // state looked like.
                    if last_ignored.elapsed() > std::time::Duration::from_secs(1) {
                        last_ignored = std::time::Instant::now();
                        rt_log(&format!(
                            "click seen while armed but not a capture gesture (ctrl={}, shift={})",
                            ctrl, shift
                        ));
                    }
                }
            }
        }
    });
}

/// Leave entry mode and go back to standby, where the machine is usable.
fn back_to_standby(app: &AppHandle) {
    set_overlay_interactive(app, false);
    let _ = app.emit("entry-closed", ());
    rt_log("entry: closed, back to standby");
}

/// The overlay JS calls this once its armed UI is on screen.
#[tauri::command]
fn overlay_ready() {
    rt_log("overlay_ready received from webview");
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
    target: Option<String>,
) -> Result<CaptureResult, String> {
    let pending = {
        let state: State<AppState> = app.state();
        let mut guard = state.pending.lock().unwrap();
        guard.take().ok_or("no pending tag to save")?
    };
    let mut tag = pending.tag;
    tag.text = text.clone();
    tag.severity = severity.clone();
    tag.area = area.clone();
    // An overlay that has not learned about targets yet keeps the one the
    // URL host suggested.
    let target = target.unwrap_or_else(|| tag.target.clone());
    tag.target = target.clone();
    let number = tag.number;

    let store = SweepStore::new(sweeps_dir());
    store
        .append_tag(&pending.sweep_name, tag)
        .map_err(|e| e.to_string())?;

    {
        // Ctrl+Shift+A attaches to this one, and Ctrl+Up recalls what was
        // just typed. Both are single slots on purpose.
        let state: State<AppState> = app.state();
        *state.last_saved.lock().unwrap() = Some((pending.sweep_name.clone(), number));
        *state.last_note.lock().unwrap() = Some(LastNote {
            text,
            severity,
            area,
            target,
        });
    }

    // Saved: hand the screen straight back to the operator, still armed
    // for the next tag.
    back_to_standby(&app);

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
        if let Some(image) = &p.tag.image {
            let png = sweeps_dir().join(&p.sweep_name).join(image);
            let _ = std::fs::remove_file(png);
        }
    }
    back_to_standby(&app);
    Ok(())
}

/// The overlay reports where it drew the pen chip, in its own CSS pixels.
/// The conversion to screen pixels happens here, with the monitor the hint
/// was placed on, so the hook can hit test the click.
#[tauri::command]
fn set_chip_rect(app: AppHandle, x: f64, y: f64, w: f64, h: f64) {
    let monitor = {
        let state: State<AppState> = app.state();
        let v = *state.chip_monitor.lock().unwrap();
        v
    };
    let (origin, scale) = monitor.unwrap_or(((0, 0), 1.0));
    hook::set_chip_rect(hook::chip_rect_from_css(origin, scale, x, y, w, h));
}

#[tauri::command]
fn clear_chip_rect() {
    hook::clear_chip_rect();
}

/// The single recall slot behind Ctrl+Up in the popover.
#[tauri::command]
fn get_last_note(state: State<AppState>) -> Option<LastNote> {
    state.last_note.lock().unwrap().clone()
}

/// Arm a one shot capture whose crop joins an existing tag instead of
/// starting a new one. Used by Ctrl+Shift+A ("compare") and by the review
/// window's Capture after button ("after").
#[tauri::command]
fn attach_next_capture(app: AppHandle, number: u32, label: String) -> Result<(), String> {
    let armed = {
        let state: State<AppState> = app.state();
        let v = *state.armed.lock().unwrap();
        v
    };
    if !armed {
        // The review window can ask for this with the app disarmed; the
        // operator meant to capture, so arm rather than refuse.
        apply_armed(&app, true);
    }
    {
        let state: State<AppState> = app.state();
        *state.attach.lock().unwrap() = Some(AttachTarget {
            sweep_name: None,
            number,
            label,
        });
    }
    hook::arm_one_shot();
    rt_log(&format!("one shot: next drag attaches to tag {}", number));
    let _ = app.emit(
        "one-shot",
        serde_json::json!({
            "on": true,
            "message": format!("drag to attach to tag {}", number),
        }),
    );
    Ok(())
}

/// Create a sweep and make it the active one. The name is sanitized into a
/// slug and dated by the store.
#[tauri::command]
fn create_sweep(app: AppHandle, name: String) -> Result<String, String> {
    let store = SweepStore::new(sweeps_dir());
    let (dir_name, _) = store
        .create_sweep(&name, &now_utc())
        .map_err(|e| e.to_string())?;
    store
        .set_active_sweep(&dir_name)
        .map_err(|e| e.to_string())?;
    rt_log(&format!("sweep: created {}", dir_name));
    let _ = app.emit("sweeps-changed", dir_name.clone());
    Ok(dir_name)
}

/// The new sweep prompt closes itself through here, so the window keeps no
/// capability of its own.
#[tauri::command]
fn close_new_sweep(app: AppHandle) {
    if let Some(w) = app.get_webview_window("newsweep") {
        let _ = w.close();
    }
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
    target: Option<String>,
) -> Result<(), String> {
    let store = SweepStore::new(sweeps_dir());
    // The review window learns to send a target in Phase C. Until then an
    // absent one leaves the tag's own target alone: capture reads it from
    // the URL host, and an edit of the note must not throw that away.
    let target = match target {
        Some(t) => t,
        None => store
            .load_sweep(&dir_name)
            .ok()
            .and_then(|s| s.tags.iter().find(|t| t.number == number).map(|t| t.target.clone()))
            .unwrap_or_default(),
    };
    store
        .update_tag(&dir_name, number, &text, &severity, &area, &target)
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

/// Write the five renderings into the sweep folder, plus a filtered copy
/// into every configured target export directory, and return the clipboard
/// pointer with the folders written. Nothing touches disk until the
/// operator presses Export, which calls this.
#[tauri::command]
fn export_sweep(dir_name: String) -> Result<tagfix::export::ExportResult, String> {
    let settings = settings::load(&exe_dir());
    tagfix::export::export_sweep_files(&sweeps_dir(), &dir_name, &settings.targets)
        .map_err(|e| e.to_string())
}

/// A file name that cannot climb out of the sweeps folder. Every command
/// below takes names from a webview, so none of them are trusted.
fn safe_name(name: &str) -> Result<&str, String> {
    let trimmed = name.trim();
    if trimmed.is_empty()
        || trimmed.contains('/')
        || trimmed.contains('\\')
        || trimmed.contains("..")
        || trimmed.contains(':')
    {
        return Err(format!("bad name: {}", name));
    }
    Ok(trimmed)
}

/// Render any of the five exports from sweep.json on the fly. Nothing is
/// written: Copy for chat has no reason to leave a file behind.
#[tauri::command]
fn render_export(dir_name: String, file_name: String) -> Result<String, String> {
    let dir_name = safe_name(&dir_name)?.to_string();
    let file_name = safe_name(&file_name)?.to_string();
    let root = sweeps_dir();
    let sweep = SweepStore::new(root.clone())
        .load_sweep(&dir_name)
        .map_err(|e| e.to_string())?;
    let dir = root.join(&dir_name);
    tagfix::export::render_named(&sweep, &dir_name, &file_name, |img| {
        std::fs::read(dir.join(img)).ok()
    })
    .ok_or_else(|| format!("not an export file: {}", file_name))
}

/// Open one export in whatever the shell has registered for it, rendering
/// first when the file is missing or older than the sweep it describes.
#[tauri::command]
fn open_export(dir_name: String, file_name: String) -> Result<String, String> {
    let dir_name = safe_name(&dir_name)?.to_string();
    let file_name = safe_name(&file_name)?.to_string();
    if !tagfix::export::EXPORT_FILES.contains(&file_name.as_str()) {
        return Err(format!("not an export file: {}", file_name));
    }
    let dir = sweeps_dir().join(&dir_name);
    let path = dir.join(&file_name);
    let modified = |p: &std::path::Path| p.metadata().and_then(|m| m.modified()).ok();
    // Stale means "older than sweep.json", so an edit in the review window
    // is never opened as yesterday's rendering.
    let stale = match (modified(&path), modified(&dir.join("sweep.json"))) {
        (Some(file), Some(sweep)) => file < sweep,
        _ => true,
    };
    if stale {
        let text = render_export(dir_name.clone(), file_name.clone())?;
        std::fs::write(&path, text).map_err(|e| e.to_string())?;
    }
    shell_open(&path)?;
    Ok(path.display().to_string())
}

/// ShellExecuteW, the same call Explorer makes on a double click.
fn shell_open(path: &std::path::Path) -> Result<(), String> {
    use windows::core::HSTRING;
    use windows::Win32::UI::Shell::ShellExecuteW;
    use windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;
    let file = HSTRING::from(path.as_os_str());
    let verb = HSTRING::from("open");
    let result = unsafe { ShellExecuteW(None, &verb, &file, None, None, SW_SHOWNORMAL) };
    // ShellExecuteW returns a fake HINSTANCE; anything at or below 32 is an
    // error code rather than a handle.
    if result.0 as isize <= 32 {
        return Err(format!("could not open {}", path.display()));
    }
    Ok(())
}

/// Save one export wherever the operator points the native dialog. Async so
/// the blocking dialog never runs on the thread pumping the event loop.
#[tauri::command(async)]
fn save_export_as(
    app: AppHandle,
    dir_name: String,
    file_name: String,
) -> Result<Option<String>, String> {
    use tauri_plugin_dialog::DialogExt;
    let text = render_export(dir_name, file_name.clone())?;
    let chosen = app
        .dialog()
        .file()
        .set_file_name(&file_name)
        .blocking_save_file();
    let Some(chosen) = chosen else {
        // Cancelled: not an error, just nothing to report.
        return Ok(None);
    };
    let path = chosen.into_path().map_err(|e| e.to_string())?;
    std::fs::write(&path, text).map_err(|e| e.to_string())?;
    Ok(Some(path.display().to_string()))
}

/// One PNG from a sweep folder as base64, for the review thumbnails. Kept
/// as a command so the asset protocol stays switched off.
#[tauri::command]
fn read_image(dir_name: String, image: String) -> Result<String, String> {
    use base64::Engine as _;
    let dir_name = safe_name(&dir_name)?.to_string();
    let image = safe_name(&image)?.to_string();
    let path = sweeps_dir().join(&dir_name).join(&image);
    let bytes = std::fs::read(&path).map_err(|e| e.to_string())?;
    Ok(base64::engine::general_purpose::STANDARD.encode(bytes))
}

/// One row of the carry forward picker. Dropped tags are listed too: a
/// re-report often starts from something dropped last round.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct SweepTagSummary {
    number: u32,
    text: String,
    kind: String,
    image: Option<String>,
    dropped: bool,
}

#[tauri::command]
fn list_sweep_tags(dir_name: String) -> Result<Vec<SweepTagSummary>, String> {
    let dir_name = safe_name(&dir_name)?.to_string();
    let sweep = SweepStore::new(sweeps_dir())
        .load_sweep(&dir_name)
        .map_err(|e| e.to_string())?;
    Ok(sweep
        .tags
        .iter()
        .map(|t| SweepTagSummary {
            number: t.number,
            text: t.text.lines().next().unwrap_or("").trim().to_string(),
            kind: t.kind.clone(),
            image: t.image.clone(),
            dropped: t.dropped,
        })
        .collect())
}

/// Copy tags from an earlier sweep into another one as re-reports. Returns
/// the numbers they were given in the destination.
#[tauri::command]
fn carry_forward(
    app: AppHandle,
    source: String,
    numbers: Vec<u32>,
    dest: String,
) -> Result<Vec<u32>, String> {
    let source = safe_name(&source)?.to_string();
    let dest = safe_name(&dest)?.to_string();
    let store = SweepStore::new(sweeps_dir());
    let ts = now_utc();
    let mut carried = Vec::new();
    for number in numbers {
        let tag = store
            .carry_forward(&source, number, &dest, &ts)
            .map_err(|e| e.to_string())?;
        carried.push(tag.number);
    }
    rt_log(&format!(
        "carry forward: {} tags from {} into {}",
        carried.len(),
        source,
        dest
    ));
    let _ = app.emit("sweeps-changed", dest);
    Ok(carried)
}

/// The review window's New sweep button opens the same prompt the tray
/// item does, so there is one way to name a sweep.
#[tauri::command]
fn new_sweep_prompt(app: AppHandle) {
    open_new_sweep(&app);
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
    let parsed_quote = new_settings
        .quote_hotkey
        .parse::<Shortcut>()
        .map_err(|e| format!("invalid quote hotkey \"{}\": {}", new_settings.quote_hotkey, e))?;
    let parsed_attach = new_settings.attach_hotkey.parse::<Shortcut>().map_err(|e| {
        format!(
            "invalid attach hotkey \"{}\": {}",
            new_settings.attach_hotkey, e
        )
    })?;

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
    if old.quote_hotkey != new_settings.quote_hotkey {
        let state: State<AppState> = app.state();
        let old_shortcut = parse_hotkey_or(
            &state.quote_hotkey.lock().unwrap(),
            settings::DEFAULT_QUOTE_HOTKEY,
        );
        let _ = app.global_shortcut().unregister(old_shortcut);
        app.global_shortcut()
            .register(parsed_quote)
            .map_err(|e| e.to_string())?;
        *state.quote_hotkey.lock().unwrap() = new_settings.quote_hotkey.clone();
    }
    if old.attach_hotkey != new_settings.attach_hotkey {
        let state: State<AppState> = app.state();
        let old_shortcut = parse_hotkey_or(
            &state.attach_hotkey.lock().unwrap(),
            settings::DEFAULT_ATTACH_HOTKEY,
        );
        let _ = app.global_shortcut().unregister(old_shortcut);
        app.global_shortcut()
            .register(parsed_attach)
            .map_err(|e| e.to_string())?;
        *state.attach_hotkey.lock().unwrap() = new_settings.attach_hotkey.clone();
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
        // Round 02 added a screenful of rows and the targets table.
        .inner_size(520.0, 760.0)
        .build();
}

/// A one input prompt for a sweep name. Small, native looking, closes on
/// Esc and creates on Enter.
fn open_new_sweep(app: &AppHandle) {
    if let Some(w) = app.get_webview_window("newsweep") {
        let _ = w.show();
        let _ = w.set_focus();
        return;
    }
    let _ = WebviewWindowBuilder::new(app, "newsweep", WebviewUrl::App("newsweep.html".into()))
        .title("New sweep")
        .inner_size(360.0, 150.0)
        .resizable(false)
        .always_on_top(true)
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
        // Save as needs a native save dialog; nothing else in the plugin is
        // permitted (capabilities allow dialog:allow-save only).
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_single_instance::init(|_app, _args, _cwd| {
            // A second launch lands here in the first instance's process.
            std::thread::spawn(|| {
                message_box(
                    "TagFix is already running",
                    "TagFix is already running in the tray.\n\nCtrl+Shift+T arms it, Ctrl+Shift+R opens review and export.",
                );
            });
        }))
        .manage({
            let s = settings::load(&exe_dir());
            AppState {
                armed: Mutex::new(false),
                pending: Mutex::new(None),
                hotkey: Mutex::new(s.hotkey),
                quote_hotkey: Mutex::new(s.quote_hotkey),
                attach_hotkey: Mutex::new(s.attach_hotkey),
                last_saved: Mutex::new(None),
                attach: Mutex::new(None),
                last_note: Mutex::new(None),
                last_highlight: Mutex::new(None),
                chip_monitor: Mutex::new(None),
            }
        })
        .plugin(
            tauri_plugin_global_shortcut::Builder::new()
                .with_handler(|app, shortcut, event| {
                    dbg_log(&format!("shortcut event: {:?} state {:?}", shortcut, event.state()));
                    if event.state() != ShortcutState::Pressed {
                        return;
                    }
                    let (arm_hotkey, quote_hotkey, attach_hotkey) = {
                        let state: State<AppState> = app.state();
                        let arm = parse_hotkey(&state.hotkey.lock().unwrap().clone());
                        let quote = parse_hotkey_or(
                            &state.quote_hotkey.lock().unwrap().clone(),
                            settings::DEFAULT_QUOTE_HOTKEY,
                        );
                        let attach = parse_hotkey_or(
                            &state.attach_hotkey.lock().unwrap().clone(),
                            settings::DEFAULT_ATTACH_HOTKEY,
                        );
                        (arm, quote, attach)
                    };
                    // Esc is deliberately NOT a global shortcut: in standby
                    // the operator is using their own apps and Esc belongs
                    // to them. The overlay handles Esc while it has focus
                    // for tag entry.
                    if *shortcut == arm_hotkey {
                        toggle_armed(app);
                    } else if *shortcut == quote_hotkey {
                        // Off the main thread: grabbing the highlight waits
                        // on the clipboard, and the mouse hook is here.
                        let handle = app.clone();
                        std::thread::spawn(move || {
                            let point = {
                                let state: State<AppState> = handle.state();
                                let v = *state.last_highlight.lock().unwrap();
                                v
                            };
                            start_quote(&handle, point);
                        });
                    } else if *shortcut == attach_hotkey {
                        let (armed, last) = {
                            let state: State<AppState> = app.state();
                            let armed = *state.armed.lock().unwrap();
                            let last = state.last_saved.lock().unwrap().clone();
                            (armed, last)
                        };
                        if !armed {
                            return;
                        }
                        match last {
                            Some((sweep_name, number)) => {
                                {
                                    let state: State<AppState> = app.state();
                                    *state.attach.lock().unwrap() = Some(AttachTarget {
                                        sweep_name: Some(sweep_name),
                                        number,
                                        label: store::LABEL_COMPARE.to_string(),
                                    });
                                }
                                hook::arm_one_shot();
                                rt_log(&format!(
                                    "one shot: next drag attaches to tag {}",
                                    number
                                ));
                                let _ = app.emit(
                                    "one-shot",
                                    serde_json::json!({
                                        "on": true,
                                        "message": format!("drag to attach to tag {}", number),
                                    }),
                                );
                            }
                            None => toast(app, "no tag to attach to", 2500),
                        }
                    } else if shortcut.matches(Modifiers::CONTROL | Modifiers::SHIFT, Code::KeyR) {
                        open_review(app);
                    } else if shortcut.matches(Modifiers::CONTROL | Modifiers::SHIFT, Code::KeyS) {
                        // Trackpad friendly: no chording needed, the next
                        // plain left drag marks the region.
                        let armed = {
                            let state = app.state::<AppState>();
                            let v = *state.armed.lock().unwrap();
                            v
                        };
                        if armed {
                            hook::arm_one_shot();
                            rt_log("one shot: next drag marks a region");
                            let _ = app.emit("one-shot", true);
                        }
                    }
                })
                .build(),
        )
        .invoke_handler(tauri::generate_handler![
            set_armed,
            get_armed,
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
            set_chip_rect,
            clear_chip_rect,
            get_last_note,
            attach_next_capture,
            create_sweep,
            close_new_sweep,
            new_sweep_prompt,
            render_export,
            open_export,
            save_export_as,
            read_image,
            list_sweep_tags,
            carry_forward,
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
            let startup = settings::load(&exe_dir());
            let hotkey_raw = startup.hotkey.clone();
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
            // Ctrl+Shift+S: mark the next drag. On a trackpad, holding
            // modifiers through a click is awkward, so this needs none.
            if app
                .global_shortcut()
                .register(Shortcut::new(
                    Some(Modifiers::CONTROL | Modifiers::SHIFT),
                    Code::KeyS,
                ))
                .is_err()
            {
                hotkey_problems.push(
                    "The mark-next-region hotkey (ctrl+shift+s) is taken by another program. Ctrl+Shift+drag still works.".to_string(),
                );
            }
            // Quote and attach. Both have a fallback path (the pen chip,
            // and the review window), so a conflict warns and nothing more.
            if app
                .global_shortcut()
                .register(parse_hotkey_or(
                    &startup.quote_hotkey,
                    settings::DEFAULT_QUOTE_HOTKEY,
                ))
                .is_err()
            {
                hotkey_problems.push(format!(
                    "The quote hotkey ({}) is taken by another program. The pen chip after a highlight still works.",
                    startup.quote_hotkey
                ));
            }
            if app
                .global_shortcut()
                .register(parse_hotkey_or(
                    &startup.attach_hotkey,
                    settings::DEFAULT_ATTACH_HOTKEY,
                ))
                .is_err()
            {
                hotkey_problems.push(format!(
                    "The attach hotkey ({}) is taken by another program, so a comparison crop cannot be started from the keyboard.",
                    startup.attach_hotkey
                ));
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
            let arm_item =
                MenuItem::with_id(app, "arm", "Arm / disarm (Ctrl+Shift+T)", true, None::<&str>)?;
            let review_item = MenuItem::with_id(
                app,
                "review",
                "Review and export (Ctrl+Shift+R)",
                true,
                None::<&str>,
            )?;
            let quote_item = MenuItem::with_id(
                app,
                "quote",
                "Quote highlight (Ctrl+Shift+Q)",
                true,
                None::<&str>,
            )?;
            let new_sweep_item =
                MenuItem::with_id(app, "new-sweep", "New sweep", true, None::<&str>)?;
            let sweeps_item =
                MenuItem::with_id(app, "open-sweeps", "Open sweeps folder", true, None::<&str>)?;
            let settings_item =
                MenuItem::with_id(app, "settings", "Settings", true, None::<&str>)?;
            let help_item =
                MenuItem::with_id(app, "help", "How to use", true, None::<&str>)?;
            let quit_item = MenuItem::with_id(app, "quit", "Quit", true, None::<&str>)?;
            let menu = Menu::with_items(
                app,
                &[
                    &arm_item,
                    &quote_item,
                    &review_item,
                    &new_sweep_item,
                    &sweeps_item,
                    &settings_item,
                    &help_item,
                    &quit_item,
                ],
            )?;

            // Global mouse hook for the Ctrl+Shift+LeftDrag capture
            // gesture. It must be installed on the thread that pumps
            // messages, which is this one.
            let (tx, rx) = std::sync::mpsc::channel();
            match hook::install(tx) {
                Ok(()) => checkpoint("mouse hook installed"),
                Err(e) => {
                    checkpoint("mouse hook FAILED");
                    let text = format!(
                        "TagFix could not install its mouse hook ({}), so Ctrl+Shift+drag will not mark regions.\n\nSecurity software can block this.",
                        e
                    );
                    std::thread::spawn(move || message_box("TagFix hook problem", &text));
                }
            }
            spawn_hook_worker(handle.clone(), rx);

            checkpoint("tray building");
            TrayIconBuilder::with_id("main")
                .icon(app.default_window_icon().unwrap().clone())
                .tooltip("TagFix")
                .menu(&menu)
                .show_menu_on_left_click(true)
                .on_menu_event(|app, event| match event.id().as_ref() {
                    "arm" => toggle_armed(app),
                    "quote" => {
                        // Same rule as the hotkey: never on the main thread.
                        let handle = app.clone();
                        std::thread::spawn(move || {
                            let point = {
                                let state: State<AppState> = handle.state();
                                let v = *state.last_highlight.lock().unwrap();
                                v
                            };
                            start_quote(&handle, point);
                        });
                    }
                    "review" => open_review(app),
                    "new-sweep" => open_new_sweep(app),
                    "open-sweeps" => open_sweeps_folder(),
                    "settings" => open_settings(app),
                    "help" => open_help(app),
                    "quit" => {
                        hook::set_armed(false);
                        hook::uninstall();
                        app.exit(0)
                    }
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
