# TagFix manual smoke test

Run these steps on Windows 11 x64 after each phase lands. Steps accumulate as
phases are delivered.

## Phase 1: shell

1. Launch `tagfix.exe`. Expect: no visible window, tray icon appears.
2. Right-click the tray icon. Expect menu: Arm, Open sweeps folder, Settings,
   Quit.
3. Press `Ctrl+Shift+T`. Expect: red frame and ARMED badge appear.
4. Press `Esc`. Expect: frame and badge disappear.
5. While disarmed, click on a window underneath the overlay. Expect: the click
   lands on that window, not on the overlay.
6. Tray menu, Open sweeps folder. Expect: Explorer opens the sweeps directory.
7. Tray menu, Quit. Expect: process exits, tray icon disappears.

Later phases append their sections here.
