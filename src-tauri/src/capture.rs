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
#[derive(Debug, Clone, Copy)]
pub struct MonitorRegion {
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
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
