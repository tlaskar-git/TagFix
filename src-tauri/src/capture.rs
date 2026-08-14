// Region capture via Windows Graphics Capture, plus foreground window
// metadata. Windows 11 x64 only by design.

use std::path::PathBuf;
use std::sync::mpsc;

use windows::Win32::Foundation::HWND;
use windows::Win32::System::Threading::{
    OpenProcess, QueryFullProcessImageNameW, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION,
};
use windows::Win32::UI::WindowsAndMessaging::{
    GetForegroundWindow, GetWindowTextW, GetWindowThreadProcessId,
};
use windows_capture::capture::{Context, GraphicsCaptureApiHandler};
use windows_capture::frame::Frame;
use windows_capture::graphics_capture_api::InternalCaptureControl;
use windows_capture::monitor::Monitor;
use windows_capture::settings::{
    ColorFormat, CursorCaptureSettings, DirtyRegionSettings, DrawBorderSettings,
    MinimumUpdateIntervalSettings, SecondaryWindowSettings, Settings,
};

/// Physical-pixel region relative to the monitor's top left corner.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MonitorRegion {
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
}

/// A drag, normalized: top left plus size, in screen coordinates. The
/// mouse hook reports screen points, so all selection maths starts here.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Selection {
    pub screen_x: i32,
    pub screen_y: i32,
    pub width: u32,
    pub height: u32,
}

/// A rectangle in overlay CSS pixels, for drawing.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CssRect {
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
}

impl Selection {
    /// Build from two drag points in any order.
    pub fn from_drag(start: (i32, i32), end: (i32, i32)) -> Self {
        Selection {
            screen_x: start.0.min(end.0),
            screen_y: start.1.min(end.1),
            width: (end.0 - start.0).unsigned_abs(),
            height: (end.1 - start.1).unsigned_abs(),
        }
    }

    /// A stray click rather than a deliberate region.
    pub fn too_small(&self) -> bool {
        self.width < 4 || self.height < 4
    }

    /// Physical region relative to the monitor holding the selection.
    pub fn region_on(&self, monitor_origin: (i32, i32)) -> MonitorRegion {
        MonitorRegion {
            x: (self.screen_x - monitor_origin.0).max(0) as u32,
            y: (self.screen_y - monitor_origin.1).max(0) as u32,
            width: self.width,
            height: self.height,
        }
    }

    /// Where the overlay should draw the rectangle, in CSS pixels.
    pub fn css_rect(&self, monitor_origin: (i32, i32), scale: f64) -> CssRect {
        let s = if scale <= 0.0 { 1.0 } else { scale };
        CssRect {
            x: (self.screen_x - monitor_origin.0) as f64 / s,
            y: (self.screen_y - monitor_origin.1) as f64 / s,
            w: self.width as f64 / s,
            h: self.height as f64 / s,
        }
    }
}

pub struct ForegroundInfo {
    pub window_title: String,
    pub process_name: String,
}

/// Title and process of the current foreground window. Called before the
/// overlay takes focus so it reflects the app under test.
pub fn foreground_info() -> ForegroundInfo {
    unsafe {
        let hwnd: HWND = GetForegroundWindow();
        if hwnd.0.is_null() {
            return ForegroundInfo {
                window_title: String::new(),
                process_name: String::new(),
            };
        }
        let mut title_buf = [0u16; 512];
        let len = GetWindowTextW(hwnd, &mut title_buf);
        let window_title = String::from_utf16_lossy(&title_buf[..len as usize]);

        let mut pid = 0u32;
        GetWindowThreadProcessId(hwnd, Some(&mut pid));
        let mut process_name = String::new();
        if pid != 0 {
            if let Ok(handle) = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) {
                let mut name_buf = [0u16; 1024];
                let mut size = name_buf.len() as u32;
                if QueryFullProcessImageNameW(
                    handle,
                    PROCESS_NAME_WIN32,
                    windows::core::PWSTR(name_buf.as_mut_ptr()),
                    &mut size,
                )
                .is_ok()
                {
                    let full = String::from_utf16_lossy(&name_buf[..size as usize]);
                    process_name = full
                        .rsplit('\\')
                        .next()
                        .unwrap_or(&full)
                        .to_string();
                }
                let _ = windows::Win32::Foundation::CloseHandle(handle);
            }
        }
        ForegroundInfo {
            window_title,
            process_name,
        }
    }
}

struct ShotFlags {
    region: MonitorRegion,
    out_path: PathBuf,
    done: mpsc::Sender<Result<(), String>>,
}

struct ShotHandler {
    flags: ShotFlags,
}

impl GraphicsCaptureApiHandler for ShotHandler {
    type Flags = ShotFlags;
    type Error = Box<dyn std::error::Error + Send + Sync>;

    fn new(ctx: Context<Self::Flags>) -> Result<Self, Self::Error> {
        Ok(ShotHandler { flags: ctx.flags })
    }

    fn on_frame_arrived(
        &mut self,
        frame: &mut Frame,
        capture_control: InternalCaptureControl,
    ) -> Result<(), Self::Error> {
        let r = self.flags.region;
        let full_w = frame.width();
        let full_h = frame.height();
        let x2 = (r.x + r.width).min(full_w);
        let y2 = (r.y + r.height).min(full_h);
        if r.x >= x2 || r.y >= y2 {
            let _ = self
                .flags
                .done
                .send(Err("selection region is outside the captured monitor".into()));
            capture_control.stop();
            return Ok(());
        }

        let mut cropped = frame.buffer_crop(r.x, r.y, x2, y2)?;
        let width = x2 - r.x;
        let height = y2 - r.y;
        let data = cropped.as_nopadding_buffer()?;

        let result = write_png(&self.flags.out_path, width, height, data)
            .map_err(|e| e.to_string());
        let _ = self.flags.done.send(result);
        capture_control.stop();
        Ok(())
    }

    fn on_closed(&mut self) -> Result<(), Self::Error> {
        let _ = self.flags.done.send(Err("capture session closed early".into()));
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn drag_normalizes_direction() {
        let a = Selection::from_drag((100, 200), (300, 500));
        let b = Selection::from_drag((300, 500), (100, 200));
        assert_eq!(a, b);
        assert_eq!(a.screen_x, 100);
        assert_eq!(a.screen_y, 200);
        assert_eq!(a.width, 200);
        assert_eq!(a.height, 300);
    }

    #[test]
    fn drag_handles_negative_screen_coordinates() {
        // A monitor left of the primary has negative x.
        let s = Selection::from_drag((-1800, 100), (-1500, 400));
        assert_eq!(s.screen_x, -1800);
        assert_eq!(s.width, 300);
        assert_eq!(s.height, 300);
    }

    #[test]
    fn tiny_drags_are_rejected() {
        assert!(Selection::from_drag((10, 10), (12, 40)).too_small());
        assert!(Selection::from_drag((10, 10), (40, 12)).too_small());
        assert!(!Selection::from_drag((10, 10), (40, 40)).too_small());
    }

    #[test]
    fn region_is_relative_to_its_monitor() {
        let s = Selection::from_drag((2596, 300), (2896, 600));
        let r = s.region_on((2496, 0));
        assert_eq!(
            r,
            MonitorRegion { x: 100, y: 300, width: 300, height: 300 }
        );
    }

    #[test]
    fn region_clamps_when_selection_starts_off_monitor() {
        let s = Selection::from_drag((-20, -30), (100, 100));
        let r = s.region_on((0, 0));
        assert_eq!(r.x, 0);
        assert_eq!(r.y, 0);
    }

    #[test]
    fn css_rect_divides_by_scale() {
        let s = Selection::from_drag((100, 200), (400, 500));
        let c = s.css_rect((0, 0), 1.5);
        assert_eq!(c.x, 100.0 / 1.5);
        assert_eq!(c.y, 200.0 / 1.5);
        assert_eq!(c.w, 200.0);
        assert_eq!(c.h, 200.0);
    }

    #[test]
    fn css_rect_survives_a_bad_scale() {
        let s = Selection::from_drag((10, 10), (110, 110));
        let c = s.css_rect((0, 0), 0.0);
        assert_eq!(c.w, 100.0);
    }
}

fn write_png(path: &PathBuf, width: u32, height: u32, rgba: &[u8]) -> std::io::Result<()> {
    let file = std::fs::File::create(path)?;
    let w = std::io::BufWriter::new(file);
    let mut encoder = png::Encoder::new(w, width, height);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    let mut writer = encoder
        .write_header()
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?;
    writer
        .write_image_data(rgba)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?;
    Ok(())
}

/// GDI BitBlt fallback: slower and blind to some hardware accelerated
/// content, but works on display stacks where Windows Graphics Capture
/// stalls. Takes the region in virtual screen coordinates.
pub fn capture_region_gdi(
    screen_x: i32,
    screen_y: i32,
    width: u32,
    height: u32,
    out_path: &std::path::Path,
) -> Result<(), String> {
    use windows::Win32::Graphics::Gdi::{
        BitBlt, CreateCompatibleBitmap, CreateCompatibleDC, DeleteDC, DeleteObject, GetDC,
        GetDIBits, ReleaseDC, SelectObject, BITMAPINFO, BITMAPINFOHEADER, BI_RGB,
        DIB_RGB_COLORS, SRCCOPY,
    };
    unsafe {
        let screen_dc = GetDC(None);
        if screen_dc.is_invalid() {
            return Err("GetDC failed".into());
        }
        let mem_dc = CreateCompatibleDC(Some(screen_dc));
        let bitmap = CreateCompatibleBitmap(screen_dc, width as i32, height as i32);
        let old = SelectObject(mem_dc, bitmap.into());

        let blit = BitBlt(
            mem_dc,
            0,
            0,
            width as i32,
            height as i32,
            Some(screen_dc),
            screen_x,
            screen_y,
            SRCCOPY,
        );

        let mut result: Result<(), String> = match blit {
            Ok(()) => Ok(()),
            Err(e) => Err(format!("BitBlt failed: {}", e)),
        };

        if result.is_ok() {
            let mut info = BITMAPINFO {
                bmiHeader: BITMAPINFOHEADER {
                    biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                    biWidth: width as i32,
                    // Negative height: top-down rows.
                    biHeight: -(height as i32),
                    biPlanes: 1,
                    biBitCount: 32,
                    biCompression: BI_RGB.0,
                    ..Default::default()
                },
                ..Default::default()
            };
            let mut bgra = vec![0u8; (width * height * 4) as usize];
            let lines = GetDIBits(
                mem_dc,
                bitmap,
                0,
                height,
                Some(bgra.as_mut_ptr() as *mut core::ffi::c_void),
                &mut info,
                DIB_RGB_COLORS,
            );
            if lines == 0 {
                result = Err("GetDIBits failed".into());
            } else {
                // BGRA to RGBA in place.
                for px in bgra.chunks_exact_mut(4) {
                    px.swap(0, 2);
                    px[3] = 255;
                }
                result = write_png(&out_path.to_path_buf(), width, height, &bgra)
                    .map_err(|e| e.to_string());
            }
        }

        SelectObject(mem_dc, old);
        let _ = DeleteObject(bitmap.into());
        let _ = DeleteDC(mem_dc);
        ReleaseDC(None, screen_dc);
        result
    }
}

/// WGC capture with a hard timeout, falling back to GDI. Returns the
/// method that produced the PNG.
pub fn capture_region_with_fallback(
    monitor_device_name: &str,
    region: MonitorRegion,
    screen_x: i32,
    screen_y: i32,
    out_path: std::path::PathBuf,
) -> Result<&'static str, String> {
    let (tx, rx) = mpsc::channel();
    let name = monitor_device_name.to_string();
    let wgc_path = out_path.clone();
    std::thread::spawn(move || {
        let _ = tx.send(capture_region_png(&name, region, wgc_path));
    });

    let wgc_result = rx.recv_timeout(std::time::Duration::from_secs(5));
    match wgc_result {
        Ok(Ok(())) => Ok("wgc"),
        Ok(Err(e)) => {
            capture_region_gdi(screen_x, screen_y, region.width, region.height, &out_path)
                .map(|_| "gdi after wgc error")
                .map_err(|ge| format!("wgc failed ({}), gdi failed ({})", e, ge))
        }
        Err(_) => {
            // WGC is hung; leave its thread behind and take the GDI path.
            capture_region_gdi(screen_x, screen_y, region.width, region.height, &out_path)
                .map(|_| "gdi after wgc timeout")
                .map_err(|ge| format!("wgc timed out, gdi failed ({})", ge))
        }
    }
}

/// Capture a region of one monitor into a PNG. Blocks until the frame is
/// written. `monitor_device_name` is the Win32 device name, for example
/// \\.\DISPLAY1; falls back to the primary monitor when not found.
pub fn capture_region_png(
    monitor_device_name: &str,
    region: MonitorRegion,
    out_path: PathBuf,
) -> Result<(), String> {
    let monitor = Monitor::enumerate()
        .map_err(|e| e.to_string())?
        .into_iter()
        .find(|m| {
            m.device_name()
                .map(|n| n == monitor_device_name)
                .unwrap_or(false)
        })
        .or_else(|| Monitor::primary().ok())
        .ok_or_else(|| "no monitor found".to_string())?;

    let (tx, rx) = mpsc::channel();
    let flags = ShotFlags {
        region,
        out_path,
        done: tx,
    };
    let settings = Settings::new(
        monitor,
        CursorCaptureSettings::WithoutCursor,
        DrawBorderSettings::WithoutBorder,
        SecondaryWindowSettings::Default,
        MinimumUpdateIntervalSettings::Default,
        DirtyRegionSettings::Default,
        ColorFormat::Rgba8,
        flags,
    );

    // start() blocks the calling thread until capture_control.stop().
    ShotHandler::start(settings).map_err(|e| e.to_string())?;

    match rx.try_recv() {
        Ok(result) => result,
        Err(_) => Err("capture finished without producing a frame".to_string()),
    }
}
