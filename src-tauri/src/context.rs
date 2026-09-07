// Capture context: the things Claude kept having to reconstruct by hand
// from a bare crop. Which URL was on screen, which control the drag started
// on, and what the whole window looked like with the crop outlined.
//
// Everything here is best effort. UI Automation talks to another process
// and can block for as long as that process feels like, so every call runs
// on a throwaway thread behind a hard budget: a slow browser costs us a
// quarter second and an empty string, never the tag.

use std::path::Path;
use std::sync::mpsc;
use std::time::Duration;

use windows::core::Interface;
use windows::Win32::Foundation::{HWND, POINT, RECT};
use windows::Win32::Graphics::Dwm::{DwmGetWindowAttribute, DWMWA_EXTENDED_FRAME_BOUNDS};
use windows::Win32::Graphics::Gdi::{
    GetMonitorInfoW, MonitorFromWindow, MONITORINFO, MONITORINFOEXW, MONITOR_DEFAULTTONEAREST,
};
use windows::Win32::System::Com::{
    CoCreateInstance, CoInitializeEx, CLSCTX_INPROC_SERVER, COINIT_APARTMENTTHREADED,
};
use windows::Win32::System::Variant::VARIANT;
use windows::Win32::UI::Accessibility::{
    CUIAutomation, IUIAutomation, IUIAutomationElement, IUIAutomationValuePattern,
    TreeScope_Descendants, UIA_ControlTypePropertyId, UIA_EditControlTypeId, UIA_ValuePatternId,
};
use windows::Win32::UI::WindowsAndMessaging::GetForegroundWindow;

use crate::capture::MonitorRegion;

/// How long any single UI Automation lookup may take.
pub const UIA_BUDGET_MS: u64 = 250;
/// The longer side of a context frame after scaling.
pub const CONTEXT_MAX_SIDE: u32 = 1200;
/// Thickness of the red outline drawn on a context frame.
pub const OUTLINE_THICKNESS: u32 = 3;

/// A rectangle in virtual screen coordinates.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ScreenRect {
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
}

impl ScreenRect {
    pub fn right(&self) -> i32 {
        self.x + self.width as i32
    }
    pub fn bottom(&self) -> i32 {
        self.y + self.height as i32
    }
}

/// The outcome of an element lookup. `OwnProcess` is separate so main.rs
/// can hide the overlay for a frame and ask again, which is the only way
/// to see through our own click-through window.
#[derive(Debug, Clone, PartialEq)]
pub enum ElementHit {
    /// Rendered as `button 'Deploy'`.
    Found(String),
    /// The point landed on TagFix itself.
    OwnProcess,
    /// Nothing readable there.
    None,
}

// ---------------------------------------------------------------------------
// Pure geometry. Tested without a screen.
// ---------------------------------------------------------------------------

/// Clip `rect` to `bounds`. None when they do not overlap at all, which is
/// how a window dragged onto another monitor shows up.
pub fn clamp_rect(rect: ScreenRect, bounds: ScreenRect) -> Option<ScreenRect> {
    let x = rect.x.max(bounds.x);
    let y = rect.y.max(bounds.y);
    let right = rect.right().min(bounds.right());
    let bottom = rect.bottom().min(bounds.bottom());
    if right <= x || bottom <= y {
        return None;
    }
    Some(ScreenRect {
        x,
        y,
        width: (right - x) as u32,
        height: (bottom - y) as u32,
    })
}

/// Scale so the longer side is at most `max_side`. Never scales up: a small
/// window keeps its pixels.
pub fn scale_to_max(width: u32, height: u32, max_side: u32) -> (u32, u32, f64) {
    if width == 0 || height == 0 {
        return (width.max(1), height.max(1), 1.0);
    }
    let longer = width.max(height);
    if longer <= max_side || max_side == 0 {
        return (width, height, 1.0);
    }
    let factor = max_side as f64 / longer as f64;
    let w = ((width as f64 * factor).round() as u32).max(1);
    let h = ((height as f64 * factor).round() as u32).max(1);
    (w, h, factor)
}

/// Where the crop rectangle lands on the scaled context frame, clamped to
/// the image. None when the crop is entirely outside the frame.
pub fn outline_in_scaled(
    frame: ScreenRect,
    crop: ScreenRect,
    factor: f64,
    img_w: u32,
    img_h: u32,
) -> Option<(u32, u32, u32, u32)> {
    let local = clamp_rect(crop, frame)?;
    let rel_x = (local.x - frame.x) as f64 * factor;
    let rel_y = (local.y - frame.y) as f64 * factor;
    let w = (local.width as f64 * factor).round() as i64;
    let h = (local.height as f64 * factor).round() as i64;
    let x = rel_x.round() as i64;
    let y = rel_y.round() as i64;
    // Clamp to the image so a rounding overshoot cannot write out of bounds.
    let x = x.clamp(0, img_w as i64);
    let y = y.clamp(0, img_h as i64);
    let w = w.max(1).min(img_w as i64 - x);
    let h = h.max(1).min(img_h as i64 - y);
    if w <= 0 || h <= 0 {
        return None;
    }
    Some((x as u32, y as u32, w as u32, h as u32))
}

/// `button 'Deploy'`. An unnamed control is just its type; an untyped one
/// with a name is just the name.
pub fn render_element(control_type: &str, name: &str) -> String {
    let control_type = control_type.trim().to_lowercase();
    let name = name.trim();
    match (control_type.is_empty(), name.is_empty()) {
        (true, true) => String::new(),
        (true, false) => format!("'{}'", name),
        (false, true) => control_type,
        (false, false) => format!("{} '{}'", control_type, name),
    }
}

/// Is this Edit control the browser address bar? Chrome and Edge name it
/// exactly; Firefox names it after the current search engine.
pub fn is_address_bar(name: &str) -> bool {
    let n = name.trim().to_lowercase();
    if n.is_empty() {
        return false;
    }
    n == "address and search bar"
        || n.starts_with("search with")
        || n.contains("enter address")
        || n.contains("address and search bar")
}

// ---------------------------------------------------------------------------
// UI Automation, behind a budget.
// ---------------------------------------------------------------------------

/// Run `f` on its own thread and give up after `ms`. A stuck lookup leaks
/// that thread rather than the caller: an unresponsive browser must not
/// wedge a capture.
fn with_budget<T, F>(ms: u64, f: F) -> Option<T>
where
    T: Send + 'static,
    F: FnOnce() -> T + Send + 'static,
{
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let _ = tx.send(f());
    });
    rx.recv_timeout(Duration::from_millis(ms)).ok()
}

/// COM has to be live on whichever thread touches UI Automation. Called
/// once per throwaway thread; a second call on an already initialised
/// thread returns S_FALSE and is harmless.
fn automation() -> Option<IUIAutomation> {
    unsafe {
        let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
        CoCreateInstance(&CUIAutomation, None, CLSCTX_INPROC_SERVER).ok()
    }
}

/// The address bar text of the foreground window, or an empty string for
/// anything that is not a browser we know.
pub fn foreground_url() -> String {
    with_budget(UIA_BUDGET_MS, read_url_blocking).unwrap_or_default()
}

fn read_url_blocking() -> String {
    unsafe {
        let hwnd = GetForegroundWindow();
        if hwnd.0.is_null() {
            return String::new();
        }
        let automation = match automation() {
            Some(a) => a,
            None => return String::new(),
        };
        let root = match automation.ElementFromHandle(hwnd) {
            Ok(e) => e,
            Err(_) => return String::new(),
        };
        // Ask for Edit controls only. A browser window has a handful, so
        // this stays far cheaper than walking the whole tree.
        let value = VARIANT::from(UIA_EditControlTypeId.0);
        let condition = match automation.CreatePropertyCondition(UIA_ControlTypePropertyId, &value) {
            Ok(c) => c,
            Err(_) => return String::new(),
        };
        let found = match root.FindAll(TreeScope_Descendants, &condition) {
            Ok(f) => f,
            Err(_) => return String::new(),
        };
        let count = found.Length().unwrap_or(0);
        for i in 0..count {
            let element = match found.GetElement(i) {
                Ok(e) => e,
                Err(_) => continue,
            };
            let name = element
                .CurrentName()
                .map(|b| b.to_string())
                .unwrap_or_default();
            if !is_address_bar(&name) {
                continue;
            }
            if let Some(url) = value_of(&element) {
                let url = url.trim().to_string();
                if !url.is_empty() {
                    return url;
                }
            }
        }
        String::new()
    }
}

/// Read the ValuePattern text of an element.
fn value_of(element: &IUIAutomationElement) -> Option<String> {
    unsafe {
        let pattern = element.GetCurrentPattern(UIA_ValuePatternId).ok()?;
        let value: IUIAutomationValuePattern = pattern.cast().ok()?;
        value.CurrentValue().ok().map(|b| b.to_string())
    }
}

/// The control under a screen point, rendered for the metadata line.
pub fn element_at_point(x: i32, y: i32) -> ElementHit {
    with_budget(UIA_BUDGET_MS, move || element_at_point_blocking(x, y)).unwrap_or(ElementHit::None)
}

fn element_at_point_blocking(x: i32, y: i32) -> ElementHit {
    unsafe {
        let automation = match automation() {
            Some(a) => a,
            None => return ElementHit::None,
        };
        let element = match automation.ElementFromPoint(POINT { x, y }) {
            Ok(e) => e,
            Err(_) => return ElementHit::None,
        };
        // Our own overlay covers the whole monitor while armed, so a hit on
        // this process means the caller has to hide it and ask again.
        if let Ok(pid) = element.CurrentProcessId() {
            if pid as u32 == std::process::id() {
                return ElementHit::OwnProcess;
            }
        }
        let control_type = element
            .CurrentLocalizedControlType()
            .map(|b| b.to_string())
            .unwrap_or_default();
        let name = element
            .CurrentName()
            .map(|b| b.to_string())
            .unwrap_or_default();
        let rendered = render_element(&control_type, &name);
        if rendered.is_empty() {
            ElementHit::None
        } else {
            ElementHit::Found(rendered)
        }
    }
}

// ---------------------------------------------------------------------------
// Context frame.
// ---------------------------------------------------------------------------

/// The extended frame bounds of the foreground window, clipped to the
/// monitor it sits on. DWM bounds exclude the invisible resize border, so
/// this is the rectangle a human would call "the window".
pub fn foreground_frame_bounds() -> Option<ScreenRect> {
    unsafe {
        let hwnd = GetForegroundWindow();
        if hwnd.0.is_null() {
            return None;
        }
        frame_bounds_of(hwnd)
    }
}

fn frame_bounds_of(hwnd: HWND) -> Option<ScreenRect> {
    unsafe {
        let mut rect = RECT::default();
        DwmGetWindowAttribute(
            hwnd,
            DWMWA_EXTENDED_FRAME_BOUNDS,
            &mut rect as *mut RECT as *mut core::ffi::c_void,
            std::mem::size_of::<RECT>() as u32,
        )
        .ok()?;
        if rect.right <= rect.left || rect.bottom <= rect.top {
            return None;
        }
        let frame = ScreenRect {
            x: rect.left,
            y: rect.top,
            width: (rect.right - rect.left) as u32,
            height: (rect.bottom - rect.top) as u32,
        };

        // A maximized window overhangs its monitor by the border width;
        // capturing that overhang reads pixels that are not there.
        let monitor = MonitorFromWindow(hwnd, MONITOR_DEFAULTTONEAREST);
        let mut info = MONITORINFO {
            cbSize: std::mem::size_of::<MONITORINFO>() as u32,
            ..Default::default()
        };
        if GetMonitorInfoW(monitor, &mut info).as_bool() {
            let m = info.rcMonitor;
            let bounds = ScreenRect {
                x: m.left,
                y: m.top,
                width: (m.right - m.left) as u32,
                height: (m.bottom - m.top) as u32,
            };
            return clamp_rect(frame, bounds);
        }
        Some(frame)
    }
}

/// The Win32 device name and top left corner of the monitor the foreground
/// window sits on. The capture path needs both: the device name to pick the
/// right monitor, the origin to turn screen coordinates into monitor ones.
pub fn foreground_monitor() -> Option<(String, (i32, i32))> {
    unsafe {
        let hwnd = GetForegroundWindow();
        if hwnd.0.is_null() {
            return None;
        }
        let monitor = MonitorFromWindow(hwnd, MONITOR_DEFAULTTONEAREST);
        let mut info = MONITORINFOEXW {
            monitorInfo: MONITORINFO {
                cbSize: std::mem::size_of::<MONITORINFOEXW>() as u32,
                ..Default::default()
            },
            ..Default::default()
        };
        if !GetMonitorInfoW(monitor, &mut info as *mut MONITORINFOEXW as *mut MONITORINFO).as_bool()
        {
            return None;
        }
        let len = info
            .szDevice
            .iter()
            .position(|c| *c == 0)
            .unwrap_or(info.szDevice.len());
        let name = String::from_utf16_lossy(&info.szDevice[..len]);
        let origin = (info.monitorInfo.rcMonitor.left, info.monitorInfo.rcMonitor.top);
        Some((name, origin))
    }
}

/// Capture the window frame, scale it down and outline where the crop was.
/// Failure is always recoverable: the caller keeps the crop and skips the
/// frame.
pub fn write_context_frame(
    monitor_device_name: &str,
    monitor_origin: (i32, i32),
    frame: ScreenRect,
    crop: ScreenRect,
    out_path: &Path,
) -> Result<(), String> {
    let region = MonitorRegion {
        x: (frame.x - monitor_origin.0).max(0) as u32,
        y: (frame.y - monitor_origin.1).max(0) as u32,
        width: frame.width,
        height: frame.height,
    };
    let temp = std::env::temp_dir().join(format!(
        "tagfix-context-{}-{}.png",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0)
    ));
    crate::capture::capture_region_with_fallback(
        monitor_device_name,
        region,
        frame.x,
        frame.y,
        temp.clone(),
    )?;

    let result = (|| -> Result<(), String> {
        let (rgba, w, h) = read_png_rgba(&temp)?;
        let (dw, dh, factor) = scale_to_max(w, h, CONTEXT_MAX_SIDE);
        let mut scaled = if factor < 1.0 {
            downscale_rgba(&rgba, w, h, dw, dh)
        } else {
            rgba
        };
        if let Some(rect) = outline_in_scaled(frame, crop, factor, dw, dh) {
            draw_outline(&mut scaled, dw, dh, rect, OUTLINE_THICKNESS);
        }
        crate::capture::write_png(&out_path.to_path_buf(), dw, dh, &scaled)
            .map_err(|e| e.to_string())
    })();

    let _ = std::fs::remove_file(&temp);
    result
}

/// Read a PNG back as RGBA8. Our own captures are always RGBA, but a
/// hand-placed file might not be, so RGB is widened rather than rejected.
fn read_png_rgba(path: &Path) -> Result<(Vec<u8>, u32, u32), String> {
    let file = std::fs::File::open(path).map_err(|e| e.to_string())?;
    let decoder = png::Decoder::new(std::io::BufReader::new(file));
    let mut reader = decoder.read_info().map_err(|e| e.to_string())?;
    let mut buf = vec![0u8; reader.output_buffer_size()];
    let info = reader.next_frame(&mut buf).map_err(|e| e.to_string())?;
    let (w, h) = (info.width, info.height);
    let pixels = (w as usize) * (h as usize);
    match info.color_type {
        png::ColorType::Rgba => {
            buf.truncate(pixels * 4);
            Ok((buf, w, h))
        }
        png::ColorType::Rgb => {
            let mut out = vec![255u8; pixels * 4];
            for i in 0..pixels {
                out[i * 4] = buf[i * 3];
                out[i * 4 + 1] = buf[i * 3 + 1];
                out[i * 4 + 2] = buf[i * 3 + 2];
            }
            Ok((out, w, h))
        }
        other => Err(format!("unsupported png colour type {:?}", other)),
    }
}

/// Box average downscale. Slower than nearest neighbour and much kinder to
/// text, which is the whole point of a context frame.
fn downscale_rgba(src: &[u8], sw: u32, sh: u32, dw: u32, dh: u32) -> Vec<u8> {
    let mut out = vec![0u8; (dw as usize) * (dh as usize) * 4];
    if sw == 0 || sh == 0 || dw == 0 || dh == 0 {
        return out;
    }
    for dy in 0..dh {
        let y0 = (dy as u64 * sh as u64 / dh as u64) as u32;
        let y1 = (((dy + 1) as u64 * sh as u64 / dh as u64) as u32).max(y0 + 1).min(sh);
        for dx in 0..dw {
            let x0 = (dx as u64 * sw as u64 / dw as u64) as u32;
            let x1 = (((dx + 1) as u64 * sw as u64 / dw as u64) as u32).max(x0 + 1).min(sw);
            let (mut r, mut g, mut b, mut a, mut n) = (0u64, 0u64, 0u64, 0u64, 0u64);
            for y in y0..y1 {
                for x in x0..x1 {
                    let i = ((y as usize) * (sw as usize) + x as usize) * 4;
                    if i + 3 >= src.len() {
                        continue;
                    }
                    r += src[i] as u64;
                    g += src[i + 1] as u64;
                    b += src[i + 2] as u64;
                    a += src[i + 3] as u64;
                    n += 1;
                }
            }
            let o = ((dy as usize) * (dw as usize) + dx as usize) * 4;
            if n == 0 {
                out[o + 3] = 255;
                continue;
            }
            out[o] = (r / n) as u8;
            out[o + 1] = (g / n) as u8;
            out[o + 2] = (b / n) as u8;
            out[o + 3] = (a / n) as u8;
        }
    }
    out
}

/// A solid red rectangle outline, drawn inward from the edges so it stays
/// inside the image.
fn draw_outline(rgba: &mut [u8], img_w: u32, img_h: u32, rect: (u32, u32, u32, u32), thickness: u32) {
    let (rx, ry, rw, rh) = rect;
    let t = thickness.max(1);
    let mut put = |x: u32, y: u32| {
        if x >= img_w || y >= img_h {
            return;
        }
        let i = ((y as usize) * (img_w as usize) + x as usize) * 4;
        if i + 3 >= rgba.len() {
            return;
        }
        rgba[i] = 220;
        rgba[i + 1] = 30;
        rgba[i + 2] = 30;
        rgba[i + 3] = 255;
    };
    let x_end = rx.saturating_add(rw).min(img_w);
    let y_end = ry.saturating_add(rh).min(img_h);
    for y in ry..y_end {
        let on_top = y < ry + t;
        let on_bottom = y + t >= y_end;
        for x in rx..x_end {
            let on_left = x < rx + t;
            let on_right = x + t >= x_end;
            if on_top || on_bottom || on_left || on_right {
                put(x, y);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rect(x: i32, y: i32, width: u32, height: u32) -> ScreenRect {
        ScreenRect { x, y, width, height }
    }

    #[test]
    fn scale_leaves_small_frames_alone() {
        assert_eq!(scale_to_max(800, 600, 1200), (800, 600, 1.0));
        assert_eq!(scale_to_max(1200, 400, 1200), (1200, 400, 1.0));
    }

    #[test]
    fn scale_shrinks_by_the_longer_side() {
        let (w, h, f) = scale_to_max(2400, 1200, 1200);
        assert_eq!((w, h), (1200, 600));
        assert!((f - 0.5).abs() < 1e-9);
        // Portrait: the height decides.
        let (w, h, _) = scale_to_max(1000, 2000, 1200);
        assert_eq!((w, h), (600, 1200));
    }

    #[test]
    fn scale_never_produces_a_zero_side() {
        let (w, h, _) = scale_to_max(4000, 1, 1200);
        assert_eq!(h, 1);
        assert_eq!(w, 1200);
    }

    #[test]
    fn clamp_clips_to_the_monitor() {
        let clipped = clamp_rect(rect(-10, -10, 100, 100), rect(0, 0, 50, 50)).unwrap();
        assert_eq!(clipped, rect(0, 0, 50, 50));
    }

    #[test]
    fn clamp_returns_none_when_disjoint() {
        assert!(clamp_rect(rect(2000, 0, 100, 100), rect(0, 0, 1000, 1000)).is_none());
    }

    #[test]
    fn outline_is_relative_to_the_frame_and_scaled() {
        let frame = rect(100, 200, 2400, 1200);
        let crop = rect(340, 400, 400, 200);
        let (_, _, factor) = scale_to_max(frame.width, frame.height, 1200);
        let (x, y, w, h) = outline_in_scaled(frame, crop, factor, 1200, 600).unwrap();
        // (340-100)*0.5 = 120, (400-200)*0.5 = 100, 400*0.5 = 200.
        assert_eq!((x, y, w, h), (120, 100, 200, 100));
    }

    #[test]
    fn outline_at_scale_one_keeps_its_coordinates() {
        let frame = rect(0, 0, 800, 600);
        let crop = rect(10, 20, 30, 40);
        let (x, y, w, h) = outline_in_scaled(frame, crop, 1.0, 800, 600).unwrap();
        assert_eq!((x, y, w, h), (10, 20, 30, 40));
    }

    #[test]
    fn outline_clamps_a_crop_that_overhangs_the_frame() {
        let frame = rect(0, 0, 800, 600);
        let crop = rect(700, 500, 400, 400);
        let (x, y, w, h) = outline_in_scaled(frame, crop, 1.0, 800, 600).unwrap();
        assert_eq!((x, y), (700, 500));
        assert_eq!((w, h), (100, 100));
        assert!(x + w <= 800 && y + h <= 600);
    }

    #[test]
    fn outline_is_none_when_the_crop_is_on_another_window() {
        let frame = rect(0, 0, 800, 600);
        let crop = rect(900, 900, 100, 100);
        assert!(outline_in_scaled(frame, crop, 1.0, 800, 600).is_none());
    }

    #[test]
    fn element_rendering_matches_the_brief() {
        assert_eq!(render_element("button", "Deploy"), "button 'Deploy'");
        assert_eq!(render_element("Button", " Deploy "), "button 'Deploy'");
        assert_eq!(render_element("edit", ""), "edit");
        assert_eq!(render_element("", "Deploy"), "'Deploy'");
        assert_eq!(render_element("", ""), "");
    }

    #[test]
    fn address_bar_names_are_recognised() {
        assert!(is_address_bar("Address and search bar"));
        assert!(is_address_bar("address and search bar"));
        assert!(is_address_bar("Search with Google or enter address"));
        assert!(is_address_bar("Search with DuckDuckGo"));
        assert!(!is_address_bar("Find in page"));
        assert!(!is_address_bar(""));
    }

    #[test]
    fn downscale_averages_a_solid_block() {
        // Four red pixels down to one stays red.
        let src = vec![255, 0, 0, 255].repeat(4);
        let out = downscale_rgba(&src, 2, 2, 1, 1);
        assert_eq!(out, vec![255, 0, 0, 255]);
    }

    #[test]
    fn outline_paints_the_border_and_not_the_middle() {
        let mut img = vec![0u8; 10 * 10 * 4];
        draw_outline(&mut img, 10, 10, (2, 2, 6, 6), 1);
        let px = |x: usize, y: usize| img[(y * 10 + x) * 4];
        assert_eq!(px(2, 2), 220);
        assert_eq!(px(7, 7), 220);
        assert_eq!(px(5, 5), 0);
        // Nothing outside the rectangle.
        assert_eq!(px(1, 1), 0);
        assert_eq!(px(8, 8), 0);
    }
}
