# TagFix manual smoke test

Run these steps on Windows 11 x64 after each phase lands. Steps accumulate as
phases are delivered.

## Phase 1: shell

1. Launch `tagfix.exe`. Expect: no visible window, tray icon appears (drag it
   out of the tray overflow on first run if Windows hides it).
2. Right-click the tray icon. Expect menu: Arm, Review and export, Open
   sweeps folder, Settings, Quit.
3. Press `Ctrl+Shift+T`. Expect: red frame and ARMED badge appear.
4. Press `Esc`. Expect: frame and badge disappear.
5. While disarmed, click on a window underneath the overlay. Expect: the
   click lands on that window, not on the overlay.
6. Tray menu, Open sweeps folder. Expect: Explorer opens the sweeps
   directory.
7. Tray menu, Quit. Expect: process exits, tray icon disappears.

## Phase 2: capture

1. Arm. Expect: crosshair cursor.
2. Drag a box over a window. Expect: red selection rectangle follows the
   drag.
3. Release. Expect: a popover opens; a `tag-NN.png` appears in the active
   sweep folder containing exactly the dragged region, with no TagFix
   chrome (no red frame, badge, or selection box) in the pixels.
4. On a machine with a secondary monitor at 150 percent scaling: move the
   cursor to that monitor, arm, drag a region. Expect: pixel-correct PNG and
   a region rect in sweep.json matching the physical pixels.
5. Check sweep.json. Expect per tag: capturedUtc, monitorIndex, dpiScale,
   region, windowTitle, processName, screenResolution.

## Phase 3: tag entry

1. Capture a region. Expect: popover near the region with text field,
   severity chips (high, medium, low) and area chips (layout, copy, a11y,
   behaviour, other), medium and other preselected.
2. Type a note, press `Enter`. Expect: popover closes, badge counter
   advances, still armed for the next tag.
3. Capture another region, press `Shift+Enter` inside the text field.
   Expect: newline, no save.
4. Press `Esc` with the popover open. Expect: popover closes, no tag saved,
   the pending PNG is deleted, still armed.
5. Speed check: five tags in under sixty seconds using only region drags and
   typing.

## Phase 4: sweep store

1. `tagfix sweep new my-slug` then `tagfix sweep list`. Expect: the new
   sweep listed with 0 tags.
2. Capture two tags, kill the process from Task Manager mid-typing on a
   third. Relaunch. Expect: the first two tags intact, only the in-flight
   tag lost.

## Phase 5: review and export

1. Tray menu, Review and export. Expect: tags listed in order.
2. Drag a row to a new position, close and reopen the window. Expect: order
   kept.
3. Edit text, change severity, drop a tag. Expect: changes persist;
   the dropped tag stays in sweep.json with `"dropped": true`.
4. Press Export. Expect: fixlist.md, fixlist.html, brief.md in the sweep
   folder and a pointer to brief.md on the clipboard.
5. Open fixlist.html in a browser with the network cable pulled (or devtools
   offline). Expect: every image visible, zero external requests.

## Phase 6: settings and packaging

1. Tray menu, Settings. Change the hotkey to `ctrl+alt+f9`, save. Expect:
   old hotkey dead, new hotkey arms.
2. Set an output directory, save, capture a tag. Expect: sweep lands there.
3. Toggle launch at login, save. Expect: `TagFix` value appears under
   HKCU\Software\Microsoft\Windows\CurrentVersion\Run, and disappears when
   toggled off.
4. Copy `tagfix.exe` alone to a fresh folder (no repo, no target dir) and
   run it as a non-admin user. Expect: it runs, captures, and exports.
