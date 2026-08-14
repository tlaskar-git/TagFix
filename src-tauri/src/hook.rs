// Global mouse hook: watches for Ctrl+Shift+LeftDrag while TagFix is
// armed, so the overlay can stay click-through and the machine stays
// usable between tags.
//
// The hook callback runs on the thread that installed it and must return
// fast, so it only sets flags and posts events down a channel. Everything
// slow happens on the worker thread that drains that channel.

use std::sync::atomic::{AtomicBool, AtomicIsize, Ordering};
use std::sync::mpsc::Sender;
use std::sync::Mutex;

use windows::Win32::Foundation::{LPARAM, LRESULT, WPARAM};
use windows::Win32::UI::Input::KeyboardAndMouse::{GetAsyncKeyState, VK_CONTROL, VK_SHIFT};
use windows::Win32::UI::WindowsAndMessaging::{
    CallNextHookEx, SetWindowsHookExW, UnhookWindowsHookEx, HHOOK, HC_ACTION, MSLLHOOKSTRUCT,
    WH_MOUSE_LL, WM_LBUTTONDOWN, WM_LBUTTONUP, WM_MOUSEMOVE,
};

#[derive(Debug, Clone, Copy)]
pub enum HookEvent {
    /// Ctrl+Shift+LeftDown at a screen point.
    Start(i32, i32),
    /// Drag in progress.
    Update(i32, i32),
    /// Button released; selection finished.
    End(i32, i32),
}

static SENDER: Mutex<Option<Sender<HookEvent>>> = Mutex::new(None);
static ARMED: AtomicBool = AtomicBool::new(false);
static SELECTING: AtomicBool = AtomicBool::new(false);
static HOOK_HANDLE: AtomicIsize = AtomicIsize::new(0);

pub fn set_armed(armed: bool) {
    ARMED.store(armed, Ordering::SeqCst);
    if !armed {
        SELECTING.store(false, Ordering::SeqCst);
    }
}

fn modifiers_held() -> bool {
    // Debug-only seam: the test harness on the build machine cannot hold
    // modifier keys across a synthetic drag, so debug builds can opt into
    // treating any left drag as the gesture. Never compiled into release.
    #[cfg(debug_assertions)]
    {
        if std::env::var("TAGFIX_TEST_ANY_DRAG").as_deref() == Ok("1") {
            return true;
        }
    }
    unsafe {
        let ctrl = GetAsyncKeyState(VK_CONTROL.0 as i32) as u16 & 0x8000 != 0;
        let shift = GetAsyncKeyState(VK_SHIFT.0 as i32) as u16 & 0x8000 != 0;
        ctrl && shift
    }
}

fn post(event: HookEvent) {
    if let Ok(guard) = SENDER.lock() {
        if let Some(tx) = guard.as_ref() {
            let _ = tx.send(event);
        }
    }
}

unsafe extern "system" fn hook_proc(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if code == HC_ACTION as i32 && ARMED.load(Ordering::SeqCst) {
        let info = &*(lparam.0 as *const MSLLHOOKSTRUCT);
        let (x, y) = (info.pt.x, info.pt.y);
        match wparam.0 as u32 {
            WM_LBUTTONDOWN => {
                if modifiers_held() && !SELECTING.load(Ordering::SeqCst) {
                    SELECTING.store(true, Ordering::SeqCst);
                    post(HookEvent::Start(x, y));
                    // Swallow it: this click marks a region, it must not
                    // reach the app underneath.
                    return LRESULT(1);
                }
            }
            WM_MOUSEMOVE => {
                if SELECTING.load(Ordering::SeqCst) {
                    post(HookEvent::Update(x, y));
                    return LRESULT(1);
                }
            }
            WM_LBUTTONUP => {
                if SELECTING.load(Ordering::SeqCst) {
                    SELECTING.store(false, Ordering::SeqCst);
                    post(HookEvent::End(x, y));
                    return LRESULT(1);
                }
            }
            _ => {}
        }
    }
    CallNextHookEx(None, code, wparam, lparam)
}

/// Install the hook. Must be called from a thread that pumps messages,
/// which is the main thread here.
pub fn install(sender: Sender<HookEvent>) -> Result<(), String> {
    if HOOK_HANDLE.load(Ordering::SeqCst) != 0 {
        return Ok(());
    }
    if let Ok(mut guard) = SENDER.lock() {
        *guard = Some(sender);
    }
    unsafe {
        match SetWindowsHookExW(WH_MOUSE_LL, Some(hook_proc), None, 0) {
            Ok(h) => {
                HOOK_HANDLE.store(h.0 as isize, Ordering::SeqCst);
                Ok(())
            }
            Err(e) => Err(e.to_string()),
        }
    }
}

pub fn uninstall() {
    let raw = HOOK_HANDLE.swap(0, Ordering::SeqCst);
    if raw != 0 {
        unsafe {
            let _ = UnhookWindowsHookEx(HHOOK(raw as *mut core::ffi::c_void));
        }
    }
}

