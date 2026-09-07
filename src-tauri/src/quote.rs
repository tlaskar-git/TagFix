// Reading the operator's highlight: there is no Win32 call that hands you
// "the text selected in the foreground app", so we borrow the clipboard for
// a moment. Snapshot what is on it, inject Ctrl+C, wait for the sequence
// number to move, read the result, put the old contents back.
//
// Two details matter and are easy to get wrong:
//   * the operator may be physically holding Ctrl+Shift while the hotkey
//     fires. Those keys must be released in the injected stream first, or
//     the app underneath sees Ctrl+Shift+C (dev tools in a browser) instead
//     of a copy. They are deliberately not re-pressed: Windows re-reads the
//     real keyboard state on the next physical event anyway.
//   * an image or a file list on the clipboard cannot be restored through
//     HGLOBAL, so we only put text and HTML back. Documented in the help
//     window rather than silently pretending.

use std::time::{Duration, Instant};

use windows::core::PCWSTR;
use windows::Win32::Foundation::{GlobalFree, HANDLE, HGLOBAL};
use windows::Win32::System::DataExchange::{
    CloseClipboard, EmptyClipboard, GetClipboardData, GetClipboardSequenceNumber,
    IsClipboardFormatAvailable, OpenClipboard, RegisterClipboardFormatW, SetClipboardData,
};
use windows::Win32::System::Memory::{GlobalAlloc, GlobalLock, GlobalSize, GlobalUnlock, GMEM_MOVEABLE};
use windows::Win32::System::Ole::CF_UNICODETEXT;
use windows::Win32::UI::Input::KeyboardAndMouse::{
    GetAsyncKeyState, SendInput, INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT, KEYBD_EVENT_FLAGS,
    KEYEVENTF_KEYUP, VIRTUAL_KEY, VK_CONTROL, VK_LCONTROL, VK_LMENU, VK_LSHIFT, VK_LWIN, VK_MENU,
    VK_RCONTROL, VK_RMENU, VK_RSHIFT, VK_RWIN, VK_SHIFT,
};

/// What the highlight grab produced. `html` is the raw CF_HTML fragment,
/// stored but not rendered this round.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Grabbed {
    pub text: String,
    pub html: Option<String>,
}

const CF_UNICODETEXT_ID: u32 = CF_UNICODETEXT.0 as u32;
const VK_C: VIRTUAL_KEY = VIRTUAL_KEY(0x43);

/// Extract the fragment CF_HTML actually describes.
///
/// CF_HTML is a text header of byte offsets followed by a full HTML
/// document; only the bytes between StartFragment and EndFragment are the
/// selection. Some apps write the header offsets, some only the comment
/// markers, some neither, so all three are tolerated. Pure, so it is
/// testable without a clipboard.
pub fn extract_html_fragment(raw: &str) -> String {
    if let (Some(start), Some(end)) = (header_offset(raw, "StartFragment:"), header_offset(raw, "EndFragment:")) {
        if start <= end && end <= raw.len() {
            if let Some(slice) = raw.get(start..end) {
                return slice.trim().to_string();
            }
        }
    }
    // Fall back to the comment markers, which sit at those same offsets.
    const START: &str = "<!--StartFragment-->";
    const END: &str = "<!--EndFragment-->";
    if let Some(s) = raw.find(START) {
        let from = s + START.len();
        if let Some(e) = raw[from..].find(END) {
            return raw[from..from + e].trim().to_string();
        }
        return raw[from..].trim().to_string();
    }
    raw.trim().to_string()
}

/// Read one `Name:00000123` header value. Returns None when the header is
/// absent or not a plain number.
fn header_offset(raw: &str, name: &str) -> Option<usize> {
    // Only the header block is scanned: a document body may quote the word.
    let head_end = raw.len().min(1024);
    let head = raw.get(..head_end)?;
    let at = head.find(name)? + name.len();
    let rest = &head[at..];
    let digits: String = rest
        .chars()
        .skip_while(|c| c.is_whitespace())
        .take_while(|c| c.is_ascii_digit())
        .collect();
    if digits.is_empty() {
        return None;
    }
    digits.parse::<usize>().ok()
}

/// Copy the current selection out of the foreground app and restore the
/// clipboard. Errors name why nothing was captured so the caller can toast.
pub fn grab_selection() -> Result<Grabbed, String> {
    let html_format = register_html_format();

    // 1. Snapshot. A failure to open here is not fatal; we just cannot
    //    restore afterwards, and say so by leaving the snapshot empty.
    let previous = snapshot(html_format);

    let before = unsafe { GetClipboardSequenceNumber() };

    // 2. Inject the copy.
    send_copy();

    // 3. Wait for the app to answer. 400 ms is generous for a local app and
    //    still short enough that a wrong guess does not feel like a hang.
    let deadline = Instant::now() + Duration::from_millis(400);
    let mut changed = false;
    while Instant::now() < deadline {
        if unsafe { GetClipboardSequenceNumber() } != before {
            changed = true;
            break;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    if !changed {
        return Err("nothing highlighted".to_string());
    }

    // 4. Read what arrived, then put the old contents back either way.
    let grabbed = snapshot(html_format);
    restore(&previous, html_format);

    let text = grabbed.text.unwrap_or_default();
    if text.trim().is_empty() {
        return Err("nothing highlighted".to_string());
    }
    let html = grabbed.html.map(|raw| extract_html_fragment(&raw));
    Ok(Grabbed { text, html })
}

/// The clipboard contents we care about: text and, when present, CF_HTML.
#[derive(Default)]
struct Snapshot {
    text: Option<String>,
    html: Option<String>,
}

fn register_html_format() -> u32 {
    let name: Vec<u16> = "HTML Format\0".encode_utf16().collect();
    unsafe { RegisterClipboardFormatW(PCWSTR(name.as_ptr())) }
}

/// Open the clipboard, retrying briefly: another app may hold it for a
/// frame or two, especially right after a copy.
fn open_clipboard() -> bool {
    for _ in 0..10 {
        if unsafe { OpenClipboard(None) }.is_ok() {
            return true;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    false
}

fn snapshot(html_format: u32) -> Snapshot {
    let mut out = Snapshot::default();
    if !open_clipboard() {
        return out;
    }
    unsafe {
        if IsClipboardFormatAvailable(CF_UNICODETEXT_ID).is_ok() {
            if let Ok(handle) = GetClipboardData(CF_UNICODETEXT_ID) {
                out.text = read_wide(handle);
            }
        }
        if html_format != 0 && IsClipboardFormatAvailable(html_format).is_ok() {
            if let Ok(handle) = GetClipboardData(html_format) {
                out.html = read_utf8(handle);
            }
        }
        let _ = CloseClipboard();
    }
    out
}

fn restore(previous: &Snapshot, html_format: u32) {
    if previous.text.is_none() && previous.html.is_none() {
        return;
    }
    if !open_clipboard() {
        return;
    }
    unsafe {
        let _ = EmptyClipboard();
        if let Some(text) = &previous.text {
            if let Some(handle) = alloc_wide(text) {
                // SetClipboardData takes ownership of the block on success.
                if SetClipboardData(CF_UNICODETEXT_ID, Some(handle)).is_err() {
                    let _ = GlobalFree(Some(HGLOBAL(handle.0)));
                }
            }
        }
        if html_format != 0 {
            if let Some(html) = &previous.html {
                if let Some(handle) = alloc_utf8(html) {
                    if SetClipboardData(html_format, Some(handle)).is_err() {
                        let _ = GlobalFree(Some(HGLOBAL(handle.0)));
                    }
                }
            }
        }
        let _ = CloseClipboard();
    }
}

unsafe fn read_wide(handle: HANDLE) -> Option<String> {
    let hglobal = HGLOBAL(handle.0);
    let ptr = GlobalLock(hglobal) as *const u16;
    if ptr.is_null() {
        return None;
    }
    let mut len = 0usize;
    // The block is NUL terminated; GlobalSize is only an upper bound.
    let max = GlobalSize(hglobal) / 2;
    while len < max && *ptr.add(len) != 0 {
        len += 1;
    }
    let slice = std::slice::from_raw_parts(ptr, len);
    let text = String::from_utf16_lossy(slice);
    let _ = GlobalUnlock(hglobal);
    Some(text)
}

unsafe fn read_utf8(handle: HANDLE) -> Option<String> {
    let hglobal = HGLOBAL(handle.0);
    let ptr = GlobalLock(hglobal) as *const u8;
    if ptr.is_null() {
        return None;
    }
    let max = GlobalSize(hglobal);
    let mut len = 0usize;
    while len < max && *ptr.add(len) != 0 {
        len += 1;
    }
    let slice = std::slice::from_raw_parts(ptr, len);
    let text = String::from_utf8_lossy(slice).to_string();
    let _ = GlobalUnlock(hglobal);
    Some(text)
}

unsafe fn alloc_bytes(bytes: &[u8]) -> Option<HANDLE> {
    let hglobal = GlobalAlloc(GMEM_MOVEABLE, bytes.len()).ok()?;
    let ptr = GlobalLock(hglobal) as *mut u8;
    if ptr.is_null() {
        let _ = GlobalFree(Some(hglobal));
        return None;
    }
    std::ptr::copy_nonoverlapping(bytes.as_ptr(), ptr, bytes.len());
    let _ = GlobalUnlock(hglobal);
    Some(HANDLE(hglobal.0))
}

unsafe fn alloc_wide(text: &str) -> Option<HANDLE> {
    let wide: Vec<u16> = text.encode_utf16().chain(std::iter::once(0)).collect();
    let bytes = std::slice::from_raw_parts(wide.as_ptr() as *const u8, wide.len() * 2);
    alloc_bytes(bytes)
}

unsafe fn alloc_utf8(text: &str) -> Option<HANDLE> {
    let mut bytes = text.as_bytes().to_vec();
    bytes.push(0);
    alloc_bytes(&bytes)
}

fn key_input(vk: VIRTUAL_KEY, flags: KEYBD_EVENT_FLAGS) -> INPUT {
    INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 {
            ki: KEYBDINPUT {
                wVk: vk,
                wScan: 0,
                dwFlags: flags,
                time: 0,
                dwExtraInfo: 0,
            },
        },
    }
}

/// Release every modifier the operator is physically holding, then send a
/// clean Ctrl+C. Nothing is re-pressed on purpose.
fn send_copy() {
    const MODIFIERS: [VIRTUAL_KEY; 11] = [
        VK_CONTROL, VK_LCONTROL, VK_RCONTROL, VK_SHIFT, VK_LSHIFT, VK_RSHIFT, VK_MENU, VK_LMENU,
        VK_RMENU, VK_LWIN, VK_RWIN,
    ];
    let mut inputs: Vec<INPUT> = Vec::with_capacity(MODIFIERS.len() + 4);
    unsafe {
        for vk in MODIFIERS {
            // The high bit means the key is down right now.
            if (GetAsyncKeyState(vk.0 as i32) as u16 & 0x8000) != 0 {
                inputs.push(key_input(vk, KEYEVENTF_KEYUP));
            }
        }
    }
    inputs.push(key_input(VK_CONTROL, KEYBD_EVENT_FLAGS(0)));
    inputs.push(key_input(VK_C, KEYBD_EVENT_FLAGS(0)));
    inputs.push(key_input(VK_C, KEYEVENTF_KEYUP));
    inputs.push(key_input(VK_CONTROL, KEYEVENTF_KEYUP));
    unsafe {
        SendInput(&inputs, std::mem::size_of::<INPUT>() as i32);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fragment_uses_header_offsets() {
        // Offsets are byte counts into the whole payload. The header is a
        // fixed width because the numbers are zero padded to eight digits.
        let head_len = "Version:0.9\r\nStartFragment:00000000\r\nEndFragment:00000000\r\n".len();
        let start = head_len + "<html><body>".len();
        let end = start + "<b>hello</b>".len();
        let raw = format!(
            "Version:0.9\r\nStartFragment:{:08}\r\nEndFragment:{:08}\r\n<html><body><b>hello</b></body></html>",
            start, end
        );
        assert_eq!(extract_html_fragment(&raw), "<b>hello</b>");
    }

    #[test]
    fn fragment_falls_back_to_comment_markers() {
        let raw = "Version:0.9\r\n<html><body><!--StartFragment--><i>x</i><!--EndFragment--></body></html>";
        assert_eq!(extract_html_fragment(raw), "<i>x</i>");
    }

    #[test]
    fn fragment_tolerates_a_missing_end_marker() {
        let raw = "<html><body><!--StartFragment--><i>x</i></body></html>";
        assert_eq!(extract_html_fragment(raw), "<i>x</i></body></html>");
    }

    #[test]
    fn fragment_without_any_marker_is_the_whole_text() {
        let raw = "  <p>plain</p>  ";
        assert_eq!(extract_html_fragment(raw), "<p>plain</p>");
    }

    #[test]
    fn fragment_ignores_out_of_range_offsets() {
        let raw = "StartFragment:00000010\r\nEndFragment:99999999\r\n<!--StartFragment--><b>ok</b><!--EndFragment-->";
        assert_eq!(extract_html_fragment(raw), "<b>ok</b>");
    }

    #[test]
    fn fragment_ignores_reversed_offsets() {
        let raw = "StartFragment:00000090\r\nEndFragment:00000010\r\n<!--StartFragment--><b>ok</b><!--EndFragment-->";
        assert_eq!(extract_html_fragment(raw), "<b>ok</b>");
    }

    #[test]
    fn header_offset_reads_zero_padded_numbers() {
        assert_eq!(header_offset("StartFragment:00000131\r\n", "StartFragment:"), Some(131));
        assert_eq!(header_offset("no header here", "StartFragment:"), None);
        assert_eq!(header_offset("StartFragment:abc\r\n", "StartFragment:"), None);
    }
}
