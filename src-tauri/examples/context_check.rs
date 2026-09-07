// Dev harness: see what the capture context reads off whatever window is
// in front. UI Automation depends on the app under the cursor, so this is
// the only honest way to check it.
//
// Usage: cargo run --release --example context_check [out-context.png]
// Bring a browser to the front, park the cursor over a control, wait three
// seconds. With a path argument it also writes a context frame outlining a
// 300x200 box around the cursor.

use std::time::Instant;

use tagfix::context::{self, ElementHit, ScreenRect};
use windows::Win32::Foundation::POINT;
use windows::Win32::UI::WindowsAndMessaging::GetCursorPos;

fn main() {
    println!("bring a window to the front and park the cursor; reading in 3 seconds");
    for n in (1..=3).rev() {
        println!("  {}", n);
        std::thread::sleep(std::time::Duration::from_secs(1));
    }

    let started = Instant::now();
    let url = context::foreground_url();
    let url_ms = started.elapsed().as_millis();
    if url.is_empty() {
        println!("url: (none read; not a browser, or the lookup timed out)");
    } else {
        println!("url: {}", url);
    }
    println!("url lookup took {} ms", url_ms);

    let mut point = POINT::default();
    let have_point = unsafe { GetCursorPos(&mut point) }.is_ok();
    if !have_point {
        eprintln!("GetCursorPos failed; skipping the element lookup");
        return;
    }
    println!("cursor at {},{}", point.x, point.y);

    let started = Instant::now();
    match context::element_at_point(point.x, point.y) {
        ElementHit::Found(text) => println!("element: {}", text),
        ElementHit::OwnProcess => println!("element: (TagFix itself; the caller would hide and retry)"),
        ElementHit::None => println!("element: (nothing readable)"),
    }
    println!("element lookup took {} ms", started.elapsed().as_millis());

    match context::foreground_frame_bounds() {
        Some(frame) => println!(
            "window frame: {},{} {}x{}",
            frame.x, frame.y, frame.width, frame.height
        ),
        None => println!("window frame: (not readable)"),
    }

    let out = std::env::args().nth(1);
    if let (Some(out), Some(frame), Some((device, origin))) = (
        out,
        context::foreground_frame_bounds(),
        context::foreground_monitor(),
    ) {
        let crop = ScreenRect {
            x: point.x - 150,
            y: point.y - 100,
            width: 300,
            height: 200,
        };
        println!("monitor {} at {},{}", device, origin.0, origin.1);
        match context::write_context_frame(&device, origin, frame, crop, std::path::Path::new(&out))
        {
            Ok(()) => println!("context frame written to {}", out),
            Err(e) => eprintln!("context frame failed: {}", e),
        }
    }
}
