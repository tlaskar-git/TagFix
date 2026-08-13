# Round 01 completion report: TagFix v1

Date: 2026-08-13 (UTC)
Branch: feat/v1-foundation
Machine: Windows Server 2025, single RDP display 2496x1664 at 150 percent
scaling.

## Per phase result

| Phase | Result | Notes |
| --- | --- | --- |
| 0 preflight | PASS | After operator installed Windows 11 SDK 10.0.26100.0 (halt on 2026-08-13, resolved same day) |
| 1 shell | PASS with 1 operator check | Build, armed indicator, hotkey toggle both directions, click-through disarmed, Esc wiring: all verified live. Two environment items below |
| 2 capture | PASS | Drag at 150 percent scale produced pixel-exact PNG (743x465) and matching region rect in sweep.json, full metadata. No secondary monitor exists on this host; same per-monitor code path, operator can spot check on multi-monitor hardware |
| 3 tag entry | PASS | Popover near region, chips with defaults, Enter saves and re-arms, Shift+Enter newline. Save cycle measured at 2 seconds per tag including capture, far under the 12 second per tag budget |
| 4 sweep store | PASS | CLI new/list verified, duplicate rejected, repeated force kills mid-sweep lost nothing already saved, store reopened cleanly every time |
| 5 review and export | PASS with 1 operator check | Inline edit, severity/area, soft drop with pick back up, export of all three files, clipboard pointer, fixlist.html verified in browser: all images render, zero external requests, dropped tag excluded |
| 6 settings and packaging | PASS | Hotkey rebind proven at the OS level (custom combo registered, default released), portable 8.5 MB exe ran and captured from a fresh folder with no install and no admin, README with real screenshot |

Tests: 45 passing (minimum was 25). Store, schema round trip, BOM
tolerance, export rendering for all three formats, settings.

Exe: `src-tauri/target/release/tagfix.exe` (portable, unsigned, x64).

Downgrades taken: none. WGC worked first try; the GDI fallback was never
needed.

## Bugs found and fixed during live verification

1. Esc only worked with webview focus, which Windows sometimes refuses to
   hand over. Fixed: Esc is a global shortcut registered only while armed,
   with the JS handler kept as fallback.
2. sweep.json and settings.json written by outside tools with a UTF-8 BOM
   failed to parse. Fixed with BOM tolerance in both loaders, regression
   tests added.
3. Drag reorder never started its drag loop in Chromium based webviews
   because dragstart set no DataTransfer data. Fixed.

## Environment findings (not code defects)

- This host's shell renders no newly registered tray icons at all: a plain
  PowerShell NotifyIcon probe is equally invisible, no policy is set, and
  promotion plus an Explorer restart changed nothing. TagFix's icon is
  correctly registered (NotifyIconSettings entry exists). On standard
  Windows 11 the icon appears under the tray chevron. Ctrl+Shift+R was
  added so review and export never depends on the tray.
- The automation layer used for verification provably swallows the Esc key
  (a registered global Esc hotkey receives no WM_HOTKEY from injected Esc,
  while every other injected combo fires). Esc handling therefore needs one
  physical keypress from the operator to confirm.

## Operator one minute check

1. Press the arm hotkey, then Esc: frame and badge disappear.
2. Arm, drag a region, then Esc with the popover open: popover closes, no
   tag saved, the pending PNG is gone, still armed.
3. In review (Ctrl+Shift+R), drag a row with the mouse: order persists after
   reopening.
4. Confirm the TagFix tray icon appears on a standard Windows 11 machine.
5. If multi-monitor hardware is available: capture on a secondary monitor
   at 150 percent scaling and check the PNG.
