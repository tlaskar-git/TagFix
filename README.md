# TagFix

Tag what is wrong on screen, get a fix list out.

TagFix is a Windows 11 desktop overlay for visual QA sweeps, for what you
read as well as what you see. Arm it, drag a box around anything that looks
wrong or highlight any text that reads wrong, type a note, keep going. When
the sweep is done, put the whole set on the clipboard ready to paste into a
chat window, or export it as markdown, a single-file HTML evidence ledger,
and an agent brief with one task and one acceptance criterion per tag. No
cloud, no accounts, no telemetry: everything lives in JSON files and PNGs on
your disk.

TagFix is not an audio metadata repair tool. Different itch entirely.

**Tagging:** arm once, then keep working. When something looks wrong,
Ctrl+Shift+drag a box around it, describe it, press Enter, and the screen
is yours again.

![Armed TagFix overlay: a region of a File Explorer window has been
captured and the tag entry popover is open with severity high and area
layout selected](docs/screenshot-tagging.png)

**Review and export:** reorder, edit, drop, then export the fix list.

![TagFix review window listing three captured tags with severity and area
controls and an Export fix list button](docs/screenshot-review.png)

## Vocabulary

- **tag**: one captured item (a note plus evidence plus metadata)
- **region tag**: a screen crop plus a note
- **quote tag**: highlighted text plus a note, no pixels by default
- **context frame**: the whole window at reduced size with the crop
  outlined in red, saved beside a crop
- **attachment**: an extra crop added to an existing tag
- **carried tag**: a tag copied out of an earlier sweep as a re-report
- **target**: which product a tag is about, guessed from the page address
- **sweep**: one testing session containing many tags
- **fix list**: the exported document

## Install

1. Download
   [tagfix.exe](https://github.com/tlaskar-git/TagFix/raw/main/dist/tagfix.exe)
   (portable, unsigned, x64, about 10 MB). No installer, no admin rights.
   Windows SmartScreen may warn because the exe is unsigned: More info,
   Run anyway.
2. Put it in any folder you can write to. Sweeps land in a `sweeps` folder
   next to the exe unless you point the output directory elsewhere in
   Settings.
3. Run it. A TagFix icon appears in the system tray. On first run Windows
   may keep new tray icons hidden: drag the icon from the tray overflow onto
   the visible tray, or enable it under Settings, Personalization, Taskbar,
   Other system tray icons.

The exe is fully self contained: the C runtime is statically linked, so no
VC++ redistributable or any other install is needed. The only external
dependency is the WebView2 runtime, which is part of Windows 11 itself.

## Usage

1. Press `Ctrl+Shift+T` (or tray menu, Arm). Armed is a quiet standby
   state: carry on using the machine exactly as normal. There is no
   banner and no message, only a thin frame at the edge of the screen,
   and TagFix takes no clicks and no keys while it waits.
2. **Something looks wrong:** hold `Ctrl+Shift` and drag a box around it
   with the left mouse button. On release, a popover opens.
   On a laptop trackpad, where holding two keys through a click is
   awkward, press `Ctrl+Shift+S` instead: a short DRAG NOW prompt
   appears and your next ordinary drag marks the region. Along with the
   crop TagFix records a context frame, the page address and the element
   under the cursor.
3. **Something reads wrong:** highlight the text as you would before a
   copy. A small pen chip appears below right of where you let go and
   stays for four seconds: click it, or press `Ctrl+Shift+Q`. The popover
   opens with the quote in a scrollable block above the note box. A quote
   tag stores no pixels unless you turn that on in Settings. The pen chip
   click is swallowed by TagFix, so the app underneath keeps its
   selection; turn the chip off in Settings and the hotkey still works.
4. Type what is wrong. Pick severity (high, medium, low), area (layout,
   copy, a11y, behaviour, other) and target (which product this is about)
   or keep the defaults. The target is preselected from the page address
   when TagFix can read it. The note box spellchecks, and `Ctrl+Up`
   recalls the note and the chips from your last save so a run of similar
   notes is an edit rather than a retype.
5. `Enter` saves the tag and hands the screen straight back to you, still
   armed for the next one. `Shift+Enter` inserts a newline. `Esc` cancels
   the tag without saving.
6. **A second picture on the same tag:** press `Ctrl+Shift+A` right after
   a save and drag. That crop becomes an attachment on the tag you just
   saved instead of a new tag, and is exported beside the original.
7. `Ctrl+Shift+T` again disarms, after which `Ctrl+Shift+drag` does
   nothing.
8. `Ctrl+Shift+R` (or tray menu, Review and export): reorder tags by drag,
   edit text, change severity, area and target, click a thumbnail to
   enlarge it, drop tags (dropped tags stay in the sweep file and can be
   picked back up).
   - **New sweep** (also in the tray menu) asks for a name and starts a
     fresh sweep, dated and slugged for you.
   - **Carry forward** copies tags out of an earlier sweep into this one
     as re-reports, with the original picture. The earlier sweep is left
     alone. A carried row then offers **Capture after**, which takes one
     fresh crop and stores it beside the original, so the export shows
     before and after side by side.
   - **Copy for chat** puts the whole sweep on the clipboard as Markdown,
     ready to paste into a chat window. **Copy as text** does the same in
     plain text. Neither needs an export first.
   - **Open** opens any of the five exported files in its default app,
     rendering it first if it is missing or older than the sweep.
     **Save as** writes one wherever you choose.
9. Press Export. Five files land in the sweep folder:
   - `fixlist.md`: the full evidence ledger, images by relative path
   - `fixlist.html`: single file, images inlined, opens anywhere offline
   - `brief.md`: agent brief, one task with an acceptance criterion per
     tag, grouped under a heading per target
   - `feedback.md`: the chat ready sheet, quotes as blockquotes
   - `feedback.txt`: the same without the Markdown syntax

   A one line pointer to `brief.md` is copied to the clipboard. If a
   target has an export directory set in Settings, Export also writes
   that target's own tags and images into a folder there named after the
   sweep.

Only `Ctrl+Shift`+left click and a click on the pen chip are intercepted,
and only while armed. Every other key and click, `Esc` included, belongs
to your own apps.

### Hotkeys

| Keys | Meaning |
| --- | --- |
| `Ctrl+Shift+T` | arm or disarm (changeable in Settings) |
| `Ctrl+Shift`+drag | mark a region to tag |
| `Ctrl+Shift+S` | the next plain drag marks a region (trackpad friendly) |
| `Ctrl+Shift+Q` | turn the current highlight into a quote tag (changeable) |
| `Ctrl+Shift+A` | the next drag attaches a comparison crop to the last tag (changeable) |
| `Ctrl+Shift+R` | review and export |
| `Enter` | save the tag (in the popover) |
| `Shift+Enter` | newline (in the popover) |
| `Ctrl+Up` | recall the last note and chips (in the popover) |
| `Esc` | cancel the tag (in the popover) |

## CLI

```
tagfix sweep new <slug>    create a sweep folder for today
tagfix sweep list          list sweeps with tag counts
tagfix diag                machine report (WebView2, monitors, hotkeys),
                           also written to tagfix-diag.txt next to the exe
```

## If something misbehaves

TagFix is single instance: launching it again just tells you it is
already running. If another program owns the arm, quote or attach hotkey,
TagFix starts anyway, warns you, and you can pick different combinations
in Settings. A hotkey that failed to register is the only thing lost: the
pen chip and the review window keep working without theirs.

**The clipboard and a quote tag.** Reading a highlight means borrowing
the clipboard for a moment: TagFix saves what is there, sends Ctrl+C to
the app in front, reads the result and puts your text back. Text and HTML
come back. An image or a copied file does not, and TagFix does not pretend
otherwise: if that was on the clipboard when you quoted something, it is
gone.

**Terminals.** In a terminal with nothing selected, Ctrl+C is an
interrupt, and the quote gesture sends exactly that. Highlight the text
first. This is why quoting is a deliberate gesture rather than something
that happens on every selection.

**The page address.** The URL is read through UI Automation from Chrome,
Edge and Firefox only, and never for longer than 250 ms. Anywhere else it
stays empty, and so does the target unless you pick one in the popover.

On first launch TagFix opens a How to use window with every hotkey and the
full flow; reopen it any time from the tray menu. If arming ever fails to
draw the overlay, TagFix disarms itself within six seconds and says so
instead of covering the screen. If startup itself cannot finish, for
example because the WebView2 runtime is missing, broken, or blocked by
security software, TagFix exits with an explanation after 20 seconds
rather than hanging. On Windows 10, or any machine where TagFix reports
WebView2 missing, install the WebView2 Evergreen runtime from
https://developer.microsoft.com/microsoft-edge/webview2 first. Fatal
errors are written to tagfix-error.log next to the exe. When reporting a
problem, run `tagfix diag` and include tagfix-diag.txt, plus
tagfix-startup.log and tagfix-runtime.log if they exist. Those two trace
startup and the arm/capture path step by step, so a hang names the step
it happened in.

Screen capture uses Windows Graphics Capture. If that stalls or errors on
a machine, TagFix falls back to a GDI BitBlt grab automatically, which
loses hardware accelerated window content but works everywhere. The
runtime log records which method produced each PNG.

## Data layout

```
sweeps/
  active.txt              the sweep folder currently being written to
  2026-09-07-round-98/
    sweep.json            sweep metadata plus ordered tag array
                          (schemaVersion 2)
    tag-01.png            captured region pixels
    tag-01-context.png    the window at reduced size, crop outlined in red
    tag-01-a1.png         an attachment (a2, a3 for the next ones)
    tag-02-before.png     the original picture of a carried tag
    tag-03-quote.png      the optional window shot of a quote tag
    fixlist.md            all five appear on export
    fixlist.html
    brief.md
    feedback.md
    feedback.txt
```

A tag in `sweep.json` carries `number`, `capturedUtc`, `monitorIndex`,
`dpiScale`, `region`, `windowTitle`, `processName`, `screenResolution`,
`text`, `severity`, `area`, `dropped`, and from schema version 2 also
`kind` (`region` or `quote`), `image`, `contextImage`, `quote`,
`quoteHtml`, `url`, `element`, `target`, `attachments` and `carriedFrom`.
Every version 2 field has a default, so a sweep written by 0.2.x loads
unchanged.

`settings.json` sits next to the exe and holds `hotkey`, `outputDir`,
`launchAtLogin`, `helpShown`, `showPenChip`, `quoteScreenshot`,
`quoteHotkey`, `attachHotkey`, `contextFrame`, `newSweepEachDay` and
`targets` (each a name, a list of hosts and an optional export
directory). Missing keys fall back to their defaults, so a settings file
written by an older version keeps working. With `newSweepEachDay` on, the
first capture on a day that has no sweep yet starts `<date>-default`
rather than extending yesterday's.

## Building from source

Requires Rust (MSVC toolchain), the Windows 11 SDK, and Node only if you
want to run the UI harness or regenerate icons. No bundler, no web
framework, no CDN.

```
cd src-tauri
cargo build --release
```

The exe lands at `src-tauri/target/release/tagfix.exe`.

The UI harness runs the vanilla JS under a small DOM shim in Node, since
this UI needs a WebView2 window no test runner can open. Run it from the
repo root with `node tests/ui/run.js`; it exits non zero if any check
fails.

## Constraints honoured

- Windows 11 x64 only
- No telemetry, no network calls, no auto-update
- No tracker features: no statuses, no assignees, no boards, no sync.
  Carried tags and after captures are evidence, never a status field.
