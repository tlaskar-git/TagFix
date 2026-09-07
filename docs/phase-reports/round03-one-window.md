# Round 03 completion report: TagFix v0.4.0

Date: 2026-09-07 (UTC)
Branch: `main` (a follow-up release, committed straight to main as v0.3.1
was)
Machine: Windows Server 2025, single RDP display 2496x1664 at 150 percent
scaling, no interactive desktop session, tray icons hidden by the shell
and synthetic input blocked.

Operator's request: "double clicking the taskbar icon opens review and
export window. All of the rest on screenshot shouldn't be separate page
and all, it should be all in 1 window may be different sections and all."

## What changed

**One window instead of four.** `ui/app.html` is a sidebar plus three
sections: Review (the whole round 02 review and export UI, carry forward
panel, Capture after, Copy for chat, Copy as text, Open, Save as, Export),
Settings (the whole settings form, targets table included) and How to use
(the whole guide). The overlay, `ui/index.html`, is untouched: it is the
transparent capture surface, not a page.

- Gone: `ui/review.html`, `ui/settings.html`, `ui/help.html`,
  `ui/help.js`, `ui/newsweep.html`, `ui/newsweep.js`, and the three
  stylesheets they carried.
- New: `ui/app.html`, `ui/app.css` (the three sheets merged, each one's
  element selectors scoped to its own section so they cannot collide) and
  `ui/app.js` (the shell: sidebar, sections, hash).
- `ui/review.js` and `ui/settings.js` are now IIFEs that bail unless their
  own section is in the document. On one page their two top level
  `const { invoke } = window.__TAURI__.core` lines were a redeclaration
  error, which would have killed every script after the first.
- Two ids moved, because review and settings each owned a `save-btn` and a
  `status`: settings now uses `settings-save-btn`, `settings-status` and
  `settings-actions`. Every other id is unchanged.

**Sections switch client side, and the hash follows.** `app.js` toggles an
`active` class and writes `location.hash`, so reloading the window comes
back to the same section. An unknown name falls back to Review at both
ends: `section_url` in Rust and `nameOf` in `app.js`.

**One entry point in the backend.** `open_main(app, section)` replaces
`open_review`, `open_settings`, `open_help` and `open_new_sweep`. It
creates the window at `WebviewUrl::App("app.html#<section>")` or, when the
window already exists, shows it, focuses it and emits `show-section`.
Ctrl+Shift+R and the tray's Review and export land on Review, tray
Settings on Settings, tray How to use and the first run on How to use.

**Tray double click.** `TrayIconBuilder::on_tray_icon_event` matches
`TrayIconEvent::DoubleClick { button: MouseButton::Left, .. }` and opens
the window at Review. `show_menu_on_left_click(false)` keeps the first
click of that double click from popping the menu, so the menu is the right
button now and a single left click does nothing. Every step writes a line
to tagfix-runtime.log: the double click, created or shown, page ready, and
the close request.

**New sweep is an inline control**, a name box beside the button in the
Review toolbar. Enter or the button calls `create_sweep` and the selector
moves to what was created. `open_new_sweep`, `close_new_sweep` and
`new_sweep_prompt` are gone; `create_sweep` is unchanged. The tray item is
gone, so the menu is now Arm / disarm, Quote highlight, Review and export,
Settings, How to use, Open sweeps folder, Quit.

**Closing hides.** The window intercepts `CloseRequested`, prevents the
close and hides itself, so reopening is instant, the selected sweep
survives, and a Capture after keeps working while the operator drags on
screen. Quit in the tray still exits.

**New command `main_window_ready`.** `app.js` calls it once a section is
on screen, which puts `main window: page ready at <section>` in the
runtime log. `app.html` loads it last, after review.js and settings.js, so
that line only appears if all three scripts parsed and ran.

Docs: README (the tray gestures and the one window, the inline New sweep,
the first run), the How to use section itself (a tray section), a Round 03
block in docs/smoke-test.md, and this report. Version 0.4.0 in
`src-tauri/Cargo.toml` and `src-tauri/tauri.conf.json`.

## Counts

| | Before | After |
| --- | --- | --- |
| Rust tests | 123 (113 lib, 10 binary) | 125 (113 lib, 12 binary) |
| UI harness checks | 74 (overlay 26, settings 10, review 38) | 85 (overlay 26, settings 11, review 48) |

The two new Rust tests cover `section_url`, including the fallback. The
new harness checks cover the initial section and hash, the sidebar
switching sections and updating the hash, `show-section` moving the
window, an unknown section falling back to Review, and the inline New
sweep: an empty name refused, a typed name reaching `create_sweep`
trimmed, the box cleared, the selector moved and the status line. The old
check "New sweep opens the prompt window" is gone with the prompt.

Both harnesses now build their element map by scanning `ui/app.html` for
ids and classes rather than hand listing them, so a renamed or dropped id
fails the harness instead of failing silently in the window. The review
harness runs review.js, settings.js and app.js in one context, which is
the page they share: a redeclaration would throw there.

## Verified on this machine, and how

- **Rust suite**: `cargo test --release` through the MSVC wrapper. 125
  passed, 0 failed.
- **UI harness**: `node tests/ui/run.js`. 85 checks, all green.
  `node --check` on every `ui/*.js` and `tests/ui/*.js`.
- **Release build**: clean, no warnings. `dist/tagfix.exe` is 10,260,992
  bytes (10,256,384 in v0.3.1).
- **Diag**: `dist\tagfix.exe diag` prints `TagFix diag 0.4.0`, WebView2
  152.0.4191.66, one monitor.
- **Startup**: `dist/tagfix.exe` launched, tagfix-startup.log reached
  `startup complete` through overlay created, hotkeys, mouse hook and tray
  built. Killed afterwards; the logs and the diag file were deleted, since
  only `dist/tagfix.exe` is tracked.
- **The one window really loads**: run in an empty folder, so `helpShown`
  is false and the first run opens the window, tagfix-runtime.log carries
  `main window: created at help` and then
  `main window: page ready at help`. That is real evidence from a real
  WebView2 window: the page loaded, the fragment survived
  `WebviewUrl::App`, the section resolved to help rather than the review
  fallback, and review.js and settings.js parsed, because app.js runs
  last.
- **Exe strings**: scanned for the user name, `C:\Users` and `D:\AI`. No hits
  for the first two. One hit for the third, `D:\AI\Claude\Project\TagFix\
  repo\src-tauri`, embedded by the Tauri context macro rather than by
  rustc, which is why `--remap-path-prefix` does not reach it. The same
  single hit is in the v0.3.1 exe already shipped in this repo, so it is
  pre-existing rather than something this round introduced. It exposes the
  checkout path, not the user name.

## Needs the operator on a real desktop

This build machine hides new tray icons and blocks synthetic input, so no
tray gesture and no window interaction was ever driven here. Everything
below is what the Round 03 section of docs/smoke-test.md is for:

- the tray double click itself, and that a single left click does nothing
- the right click menu, its new order, and each item landing on its
  section
- the sidebar: switching sections, and each section keeping its state
- the hash surviving a reload inside the window
- close hiding rather than quitting, reopen being instant, and Capture
  after still landing on the right tag across a close
- the inline New sweep control, typed and with Enter
- Quit still exiting after a close that only hid the window
- how the merged page actually looks: the sidebar, the toolbar wrapping,
  the settings form and the guide all use one stylesheet now and no
  screen was ever rendered here

## Decisions the brief left open

1. **The old pages are deleted, not left behind.** The brief named only
   `ui/newsweep.*`. `review.html`, `settings.html`, `help.html`,
   `help.js` and the three stylesheets had no remaining entry point, and
   `tauri.conf.json` bundles everything under `ui/` into the exe, so
   leaving them would ship dead pages to users.
2. **One merged stylesheet rather than three linked ones.** The three
   sheets each styled `body`, `h1`, `h2`, `button`, `select`, `table`,
   `td`, `#status`, `#save-btn` and `.hidden`. On one page that is a pile
   of collisions, so `app.css` merges them with each one's element
   selectors scoped to its own section.
3. **Settings gave up the contested ids.** `save-btn` and `status` are
   review's, because the review harness is the larger of the two and the
   review markup is what most of the round 02 checks read.
4. **help.js is gone rather than kept.** Its only job was closing its own
   window on Esc. On a shared window Esc closing everything while the
   operator edits a setting would be surprising, and Esc already closes
   the lightbox and the toolbar menus in the Review section.
5. **The first run opens the window at How to use as well as showing the
   native summary box.** The brief listed the first run help as an entry
   point, and the README has always said the first launch opens the
   guide, but round 01 only ever showed the native box. The box stays: it
   is the fallback for a machine where the webview misbehaves.
6. **A `main_window_ready` command and the script order.** The brief asked
   for a log line for the double click. The window can only be verified
   from logs on this machine, so the page reports itself ready too, and
   `app.js` is loaded last so that line also proves the section scripts
   parsed.
7. **Each section script initialises itself at load, guarded by its own
   section element,** rather than waiting to be shown. Review has to be
   listening for `sweeps-changed` whether or not it is the visible
   section, and the settings form is a single read at startup.
8. **The inline New sweep refreshes the selector itself** as well as
   relying on the backend's `sweeps-changed` event, so the selector is
   right whether or not the window heard that event.

## Not done

- **The screenshots in the README are from 0.3.x.**
  `docs/screenshot-review.png` shows the old separate review window with
  no sidebar. Rendering a new one needs a headless browser with the Tauri
  bridge shimmed, as round 02 did; it was out of this round's scope and no
  screen can be captured on this host. The caption was reworded, the
  picture was not replaced.
- **The `D:\AI\...\src-tauri` string in the exe** (see above) is
  pre-existing and unfixed; it comes from the Tauri context macro, not
  from rustc path metadata.
