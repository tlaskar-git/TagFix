// Global mouse hook: watches for Ctrl+Shift+LeftDrag while TagFix is
// armed, so the overlay can stay click-through and the machine stays
// usable between tags. Round 02 adds two more things it has to notice: a
// plain highlight gesture (drag or multi click), and a click on the pen
// chip the overlay draws afterwards.
//
// The hook callback runs on the thread that installed it and must return
// fast, so it only sets flags and posts events down a channel. Everything
// slow happens on the worker thread that drains that channel.
//
// Only three things are ever swallowed: the capture chord, the one shot
// drag, and a click on the pen chip. Highlights are reported and passed
// straight through, because that drag belongs to the app underneath.

use std::sync::atomic::{AtomicBool, AtomicI32, AtomicIsize, AtomicU32, AtomicU64, Ordering};
use std::sync::mpsc::Sender;
use std::sync::{Mutex, OnceLock};

use windows::Win32::Foundation::{LPARAM, LRESULT, WPARAM};
use windows::Win32::UI::Input::KeyboardAndMouse::{
    GetAsyncKeyState, GetDoubleClickTime, VK_CONTROL, VK_MENU, VK_SHIFT,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CallNextHookEx, SetWindowsHookExW, UnhookWindowsHookEx, HHOOK, HC_ACTION, MSLLHOOKSTRUCT,
    WH_MOUSE_LL, WM_LBUTTONDOWN, WM_LBUTTONUP, WM_MOUSEMOVE,
};

/// How far a plain left drag must travel before it counts as a highlight.
pub const HIGHLIGHT_DRAG_MIN: i32 = 8;
/// How far the pointer may wander between two clicks and still count as a
/// double click. Windows uses the system metric; a few pixels is plenty.
pub const CLICK_SLOP: i32 = 4;

#[derive(Debug, Clone, Copy)]
pub enum HookEvent {
    /// Left button went down to begin a region.
    Start(i32, i32),
    /// Drag in progress.
    Update(i32, i32),
    /// Button released; selection finished.
    End(i32, i32),
    /// A plain left drag or multi click that probably selected text. Not
    /// swallowed: the app underneath owns that gesture.
    Highlight(i32, i32),
    /// The operator clicked the pen chip. Swallowed, so the selection in
    /// the app underneath survives.
    QuoteClick,
    /// A click somewhere else while the chip was showing.
    ChipDismiss,
    /// A left click arrived while armed but did not start a region.
    /// Carries the modifier state so the log can explain why.
    Ignored { ctrl: bool, shift: bool },
}

/// The pen chip rectangle in screen pixels, as the hook needs to hit test
/// it. Zero sized means there is no chip.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct ChipRect {
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
}

static SENDER: Mutex<Option<Sender<HookEvent>>> = Mutex::new(None);
static ARMED: AtomicBool = AtomicBool::new(false);
static SELECTING: AtomicBool = AtomicBool::new(false);
/// Set by the "mark next region" hotkey: the next left drag counts as the
/// gesture with no modifiers held, which matters on a trackpad where
/// chording is awkward.
static ONE_SHOT: AtomicBool = AtomicBool::new(false);
static HOOK_HANDLE: AtomicIsize = AtomicIsize::new(0);

/// The pen chip, held as plain integers so the callback never locks.
static CHIP_ON: AtomicBool = AtomicBool::new(false);
static CHIP_X: AtomicI32 = AtomicI32::new(0);
static CHIP_Y: AtomicI32 = AtomicI32::new(0);
static CHIP_W: AtomicI32 = AtomicI32::new(0);
static CHIP_H: AtomicI32 = AtomicI32::new(0);
/// The button up that closes a swallowed chip click has to go too, or the
/// app underneath sees half a click.
static SWALLOW_NEXT_UP: AtomicBool = AtomicBool::new(false);

/// Plain (unmodified) left button tracking, for the highlight gesture.
static PLAIN_DOWN: AtomicBool = AtomicBool::new(false);
static PLAIN_X: AtomicI32 = AtomicI32::new(0);
static PLAIN_Y: AtomicI32 = AtomicI32::new(0);
static CLICK_COUNT: AtomicU32 = AtomicU32::new(0);
static CLICK_MS: AtomicU64 = AtomicU64::new(0);

/// Monotonic milliseconds since the first call. Only differences matter.
static CLOCK: OnceLock<std::time::Instant> = OnceLock::new();

fn now_ms() -> u64 {
    CLOCK.get_or_init(std::time::Instant::now).elapsed().as_millis() as u64
}

pub fn set_armed(armed: bool) {
    ARMED.store(armed, Ordering::SeqCst);
    if !armed {
        SELECTING.store(false, Ordering::SeqCst);
        ONE_SHOT.store(false, Ordering::SeqCst);
        clear_chip_rect();
        PLAIN_DOWN.store(false, Ordering::SeqCst);
        CLICK_COUNT.store(0, Ordering::SeqCst);
    }
}

pub fn arm_one_shot() {
    ONE_SHOT.store(true, Ordering::SeqCst);
}

/// The overlay tells us where it drew the chip, in screen pixels.
pub fn set_chip_rect(rect: ChipRect) {
    CHIP_X.store(rect.x, Ordering::SeqCst);
    CHIP_Y.store(rect.y, Ordering::SeqCst);
    CHIP_W.store(rect.w, Ordering::SeqCst);
    CHIP_H.store(rect.h, Ordering::SeqCst);
    CHIP_ON.store(rect.w > 0 && rect.h > 0, Ordering::SeqCst);
}

pub fn clear_chip_rect() {
    CHIP_ON.store(false, Ordering::SeqCst);
    CHIP_W.store(0, Ordering::SeqCst);
    CHIP_H.store(0, Ordering::SeqCst);
    SWALLOW_NEXT_UP.store(false, Ordering::SeqCst);
}

fn chip_rect() -> ChipRect {
    ChipRect {
        x: CHIP_X.load(Ordering::SeqCst),
        y: CHIP_Y.load(Ordering::SeqCst),
        w: CHIP_W.load(Ordering::SeqCst),
        h: CHIP_H.load(Ordering::SeqCst),
    }
}

/// Is this screen point on the chip? Half open on the right and bottom, so
/// two chips side by side could never both claim a pixel.
pub fn chip_contains(rect: ChipRect, x: i32, y: i32) -> bool {
    rect.w > 0
        && rect.h > 0
        && x >= rect.x
        && x < rect.x + rect.w
        && y >= rect.y
        && y < rect.y + rect.h
}

/// Turn the chip rectangle the overlay laid out in CSS pixels into the
/// screen pixels the hook compares against. The overlay covers exactly one
/// monitor, so its origin plus the monitor scale is the whole conversion.
pub fn chip_rect_from_css(
    origin: (i32, i32),
    scale: f64,
    x: f64,
    y: f64,
    w: f64,
    h: f64,
) -> ChipRect {
    let s = if scale <= 0.0 { 1.0 } else { scale };
    ChipRect {
        x: origin.0 + (x * s).round() as i32,
        y: origin.1 + (y * s).round() as i32,
        w: (w * s).round().max(0.0) as i32,
        h: (h * s).round().max(0.0) as i32,
    }
}

/// Did this drag travel far enough to be a text selection rather than a
/// click that wobbled?
pub fn drag_is_highlight(start: (i32, i32), end: (i32, i32)) -> bool {
    let dx = (end.0 - start.0) as i64;
    let dy = (end.1 - start.1) as i64;
    let min = HIGHLIGHT_DRAG_MIN as i64;
    dx * dx + dy * dy >= min * min
}

/// How many clicks in a row this one makes. A click too late or too far
/// from the previous one starts a new streak at one.
pub fn click_streak(previous: u32, elapsed_ms: u64, moved: i32, double_click_ms: u64) -> u32 {
    if previous > 0 && elapsed_ms <= double_click_ms && moved <= CLICK_SLOP {
        previous + 1
    } else {
        1
    }
}

fn modifier_state() -> (bool, bool) {
    unsafe {
        let ctrl = GetAsyncKeyState(VK_CONTROL.0 as i32) as u16 & 0x8000 != 0;
        let shift = GetAsyncKeyState(VK_SHIFT.0 as i32) as u16 & 0x8000 != 0;
        (ctrl, shift)
    }
}

/// The highlight gesture is only a highlight when nothing is held: with a
/// modifier down the click means something else in the app underneath.
fn any_modifier_held() -> bool {
    let (ctrl, shift) = modifier_state();
    let alt = unsafe { GetAsyncKeyState(VK_MENU.0 as i32) as u16 & 0x8000 != 0 };
    ctrl || shift || alt
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
    let (ctrl, shift) = modifier_state();
    ctrl && shift
}

fn post(event: HookEvent) {
    if let Ok(guard) = SENDER.lock() {
        if let Some(tx) = guard.as_ref() {
            let _ = tx.send(event);
        }
    }
}

/// Remember this button down for the highlight gesture, and say how many
/// clicks in a row it is.
fn track_plain_down(x: i32, y: i32) {
    let now = now_ms();
    let elapsed = now.saturating_sub(CLICK_MS.load(Ordering::SeqCst));
    let moved = (x - PLAIN_X.load(Ordering::SeqCst))
        .abs()
        .max((y - PLAIN_Y.load(Ordering::SeqCst)).abs());
    let streak = click_streak(
        CLICK_COUNT.load(Ordering::SeqCst),
        elapsed,
        moved,
        unsafe { GetDoubleClickTime() } as u64,
    );
    CLICK_COUNT.store(streak, Ordering::SeqCst);
    CLICK_MS.store(now, Ordering::SeqCst);
    PLAIN_X.store(x, Ordering::SeqCst);
    PLAIN_Y.store(y, Ordering::SeqCst);
    PLAIN_DOWN.store(true, Ordering::SeqCst);
}

unsafe extern "system" fn hook_proc(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if code == HC_ACTION as i32 && ARMED.load(Ordering::SeqCst) {
        let info = &*(lparam.0 as *const MSLLHOOKSTRUCT);
        let (x, y) = (info.pt.x, info.pt.y);
        match wparam.0 as u32 {
            WM_LBUTTONDOWN => {
                if !SELECTING.load(Ordering::SeqCst) {
                    // Either the chord, or a one-shot armed by the
                    // "mark next region" hotkey.
                    if modifiers_held() || ONE_SHOT.load(Ordering::SeqCst) {
                        SELECTING.store(true, Ordering::SeqCst);
                        PLAIN_DOWN.store(false, Ordering::SeqCst);
                        post(HookEvent::Start(x, y));
                        // Swallow it: this click marks a region, it must
                        // not reach the app underneath.
                        return LRESULT(1);
                    }
                    // The chip is drawn on a click-through window, so this
                    // is the only place its click can be taken. Swallowing
                    // it is what keeps the highlight underneath alive.
                    if CHIP_ON.load(Ordering::SeqCst) {
                        if chip_contains(chip_rect(), x, y) {
                            SWALLOW_NEXT_UP.store(true, Ordering::SeqCst);
                            post(HookEvent::QuoteClick);
                            return LRESULT(1);
                        }
                        post(HookEvent::ChipDismiss);
                    }
                    // Not the gesture: let it through, but record why so
                    // a trackpad that never reports the modifiers is
                    // visible in the log.
                    let (ctrl, shift) = modifier_state();
                    post(HookEvent::Ignored { ctrl, shift });
                    if any_modifier_held() {
                        PLAIN_DOWN.store(false, Ordering::SeqCst);
                        CLICK_COUNT.store(0, Ordering::SeqCst);
                    } else {
                        track_plain_down(x, y);
                    }
                }
            }
            WM_MOUSEMOVE => {
                if SELECTING.load(Ordering::SeqCst) {
                    // Report the drag, but NEVER swallow a move: blocking
                    // WM_MOUSEMOVE freezes the cursor itself, so the drag
                    // can never leave the point it started from.
                    post(HookEvent::Update(x, y));
                }
            }
            WM_LBUTTONUP => {
                if SWALLOW_NEXT_UP.swap(false, Ordering::SeqCst) {
                    return LRESULT(1);
                }
                if SELECTING.load(Ordering::SeqCst) {
                    SELECTING.store(false, Ordering::SeqCst);
                    ONE_SHOT.store(false, Ordering::SeqCst);
                    post(HookEvent::End(x, y));
                    return LRESULT(1);
                }
                if PLAIN_DOWN.swap(false, Ordering::SeqCst) {
                    let start = (PLAIN_X.load(Ordering::SeqCst), PLAIN_Y.load(Ordering::SeqCst));
                    let dragged = drag_is_highlight(start, (x, y));
                    let multi = CLICK_COUNT.load(Ordering::SeqCst) >= 2;
                    if dragged || multi {
                        post(HookEvent::Highlight(x, y));
                    }
                    // Deliberately not swallowed: selecting text is the
                    // operator talking to their own app.
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

#[cfg(test)]
mod tests {
    use super::*;

    fn chip(x: i32, y: i32, w: i32, h: i32) -> ChipRect {
        ChipRect { x, y, w, h }
    }

    #[test]
    fn chip_hit_test_covers_the_rectangle() {
        let r = chip(100, 200, 28, 28);
        assert!(chip_contains(r, 100, 200));
        assert!(chip_contains(r, 114, 214));
        assert!(chip_contains(r, 127, 227));
    }

    #[test]
    fn chip_hit_test_excludes_the_far_edges() {
        let r = chip(100, 200, 28, 28);
        assert!(!chip_contains(r, 99, 214));
        assert!(!chip_contains(r, 114, 199));
        assert!(!chip_contains(r, 128, 214));
        assert!(!chip_contains(r, 114, 228));
    }

    #[test]
    fn an_empty_chip_never_claims_a_click() {
        assert!(!chip_contains(ChipRect::default(), 0, 0));
        assert!(!chip_contains(chip(100, 200, 0, 28), 100, 200));
    }

    #[test]
    fn chip_hit_test_works_on_a_monitor_with_negative_coordinates() {
        let r = chip(-1800, -300, 28, 28);
        assert!(chip_contains(r, -1790, -290));
        assert!(!chip_contains(r, -1810, -290));
    }

    #[test]
    fn css_to_screen_adds_the_origin_and_the_scale() {
        let r = chip_rect_from_css((2496, 0), 1.5, 100.0, 200.0, 28.0, 28.0);
        assert_eq!(r, chip(2496 + 150, 300, 42, 42));
    }

    #[test]
    fn css_to_screen_survives_a_bad_scale() {
        let r = chip_rect_from_css((0, 0), 0.0, 10.0, 20.0, 28.0, 28.0);
        assert_eq!(r, chip(10, 20, 28, 28));
    }

    #[test]
    fn drag_threshold_is_eight_pixels() {
        assert!(!drag_is_highlight((100, 100), (107, 100)));
        assert!(drag_is_highlight((100, 100), (108, 100)));
        assert!(drag_is_highlight((100, 100), (100, 92)));
        // Diagonals count by distance, not by axis.
        assert!(!drag_is_highlight((0, 0), (5, 5)));
        assert!(drag_is_highlight((0, 0), (6, 6)));
    }

    #[test]
    fn a_still_click_is_not_a_drag() {
        assert!(!drag_is_highlight((10, 10), (10, 10)));
    }

    #[test]
    fn click_streak_counts_up_while_close_and_quick() {
        assert_eq!(click_streak(0, 0, 0, 500), 1);
        assert_eq!(click_streak(1, 200, 0, 500), 2);
        assert_eq!(click_streak(2, 200, 2, 500), 3);
    }

    #[test]
    fn click_streak_restarts_when_slow_or_far() {
        assert_eq!(click_streak(1, 900, 0, 500), 1);
        assert_eq!(click_streak(2, 100, 40, 500), 1);
    }
}
