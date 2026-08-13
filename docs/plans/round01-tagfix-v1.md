# Round 01: TagFix v1 (Windows)

## Goal

Ship a working Windows desktop overlay that captures screen regions with typed
notes and exports a fix list as markdown, single-file HTML, and an agent brief.
End to end. No cloud, no accounts, no telemetry.

## Repo

tlaskar-git/TagFix. Branch off main as `feat/v1-foundation`.

## Product one-liner

Tag what is wrong on screen, get a fix list out.

This line goes in the README first sentence. It exists to kill the assumption
that TagFix is an audio metadata repair tool.

## Vocabulary

Use these exact terms in code, UI strings, docs, and file names.

- **tag**: one captured item (a region plus a note plus metadata)
- **sweep**: one testing session containing many tags
- **fix list**: the exported document

## Stack

Locked. Do not substitute without halting and asking.

- Tauri 2, Rust backend, vanilla JS overlay UI
- No bundler, no framework, no CDN. Any web assets ship in the binary
- Windows Graphics Capture API for pixel capture
- Global hotkey via `tauri-plugin-global-shortcut`
- Store: JSON files on disk. No database
- Target: Windows 11 x64 only. No macOS, no Linux, no cross-platform stubs

## Phase 0: preflight

Verify present and report versions: `rustc`, `cargo`, `node`, MSVC build tools,
WebView2 runtime.

If any is missing, HALT and report exactly what is absent. Do not install
toolchains unattended.

Note: on this machine `python` and `python3` are Microsoft Store stubs. If any
tooling needs Python, use the `py` launcher or a direct path.

Accept: all five present, versions logged.

## Phase 1: shell

- Tauri 2 scaffold. App name TagFix, binary `tagfix.exe`
- Tray icon with menu: Arm, Open sweeps folder, Settings, Quit
- Transparent always-on-top overlay window, click-through when disarmed
- Global hotkey `Ctrl+Shift+T` toggles armed state
- Visible armed indicator, `Esc` disarms

Accept: `cargo build` succeeds, tray appears, hotkey toggles the indicator,
`Esc` disarms, and the overlay does not intercept clicks when disarmed.

## Phase 2: capture

- Armed state shows a crosshair cursor
- Drag selects a region, release captures it
- Capture via Windows Graphics Capture, save PNG into the active sweep folder
- Record per tag: UTC timestamp, monitor index, DPI scale, region rect,
  foreground window title, process name, screen resolution

Accept: dragging a region on a secondary monitor at 150 percent scaling produces
a pixel-correct PNG with a correct region rect in metadata.

Fallback if this phase stalls twice: GDI `BitBlt` region grab. It loses
hardware-accelerated window content but works everywhere. Record the downgrade
in the phase report rather than halting the build.

## Phase 3: tag entry

- After capture, a popover appears near the captured region
- Text field, severity chips (high, medium, low), area chips (layout, copy,
  a11y, behaviour, other)
- `Enter` saves and re-arms for the next tag
- `Shift+Enter` newline, `Esc` cancels the tag without saving
- Running counter shows tag number and sweep name

Accept: five tags captured in under sixty seconds, with no mouse use between
entries other than selecting each region.

## Phase 4: sweep store

- Path: `sweeps/<yyyy-mm-dd>-<slug>/` containing `sweep.json` and `tag-NN.png`
- `sweep.json` holds sweep metadata plus an ordered tag array
- Schema carries a `schemaVersion` integer field
- CLI: `tagfix sweep new <slug>`, `tagfix sweep list`

Accept: killing the process mid-sweep loses at most the tag currently being
typed. The store reopens cleanly on next launch.

## Phase 5: review and export

- Review window lists tags. Reorder by drag, edit text, change severity, delete
- Deleted tags stay in `sweep.json` marked `dropped: true` rather than being
  removed, so a later sweep is able to pick them back up
- Export writes three files into the sweep folder:
  1. `fixlist.md` - full evidence ledger, one section per tag, image referenced
     by relative path
  2. `fixlist.html` - single file, images inlined as base64, opens standalone
     with zero external requests
  3. `brief.md` - agent brief with scope, one task per tag, and an acceptance
     criterion per task derived from the tag text, severity, and area
- On export, copy a one-line pointer to `brief.md` to the clipboard
- Nothing writes to disk until the operator presses Export

Accept: a five tag sweep produces all three files. `fixlist.html` opens in a
browser with every image visible and makes no network requests.

## Phase 6: settings and packaging

- Settings: hotkey binding, output directory, launch at login
- Build a portable unsigned `tagfix.exe`. No installer
- README with the one-liner, a screenshot, install steps, and usage

Accept: the exe runs from a fresh folder on a clean machine path with no install
step and no admin rights.

## Testing

- Rust unit tests covering store read and write, schema round trip, and export
  rendering for all three output formats
- Minimum 25 passing tests at completion. Below that count, HALT
- Manual smoke steps documented in `docs/smoke-test.md`

## Constraints

- No em dashes or en dashes anywhere in source, UI strings, or docs
- No telemetry, no network calls, no auto-update in v1
- No tracker features. No statuses, no assignees, no boards, no sync
- Commits prefixed `[BOT]` with a Claude co-author trailer
- Git identity: `tlaskar-git` / `taher.laskar@gmail.com`
- Commit and push at the end of every phase
- Do not open the PR until Phase 6 passes

## Hard halt triggers

- Any toolchain missing at Phase 0
- Two failed repair attempts on the same failure inside one phase
- Test count below 25 after Phase 5
- Any phase acceptance criterion unmet
- Any request or temptation to add cloud, accounts, telemetry, or tracker
  features

## Completion

Open a PR from `feat/v1-foundation` to `main` titled:

`[BOT] TagFix v1: capture, sweep store, fix list export`

Do not merge. Operator merges only, via a merge commit (ADR-049).

Post the completion report to Telegram topic 27 via `sendDocument`. The report
lists per phase pass or fail, final test count, the exe path, and any downgrade
taken (for example the GDI fallback in Phase 2).
