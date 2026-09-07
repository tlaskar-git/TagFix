# Round 02: quote tags, richer evidence, one click export

Status: LOCKED 2026-09-07. Operator approved every decision at its
recommended default and every candidate enhancement. Version 0.3.0.
Branch `round/02-quote-tags` off main. Builder: Opus 5 agents, one per
phase, in order. Each phase ends with the full test suite green, a
release build, a [BOT] commit and a push.

## Goal

TagFix becomes useful while reading as well as while looking. Highlight
text in any app, comment on it, and at the end put the whole set on the
clipboard ready to paste into a chat window. Region tags gain the
context Claude kept having to reconstruct by hand (which window, which
URL, which element, what it looked like before). The review window
saves or opens the fix list in any format without a trip through
Explorer. Everything stays local: no network, no accounts, no tracker.

## Vocabulary

- **region tag**: a screen crop plus a note (v1 behaviour)
- **quote tag**: highlighted text plus a comment, no pixels by default
- **context frame**: a reduced size PNG of the whole foreground window
  with the region outlined in red, saved beside a region crop
- **attachment**: an extra crop added to an existing tag (a comparison
  or an "after" capture)
- **carried tag**: a tag copied from an earlier sweep into the current
  one as a re-report, keeping its original evidence
- **target**: which product a tag is about (helmsly, slobal.com,
  AgnCred, other), a chip, defaulted from the URL host
- **feedback sheet**: the chat ready rendering of a sweep in order,
  Markdown or plain text

Region tags, quote tags and carried tags share one sweep and one number
sequence.

## Operator flow after this round

1. Ctrl+Shift+T arms. Silent standby, thin frame, as today.
2. Region tag: Ctrl+Shift+drag (or Ctrl+Shift+S then drag) as today.
   The crop, a context frame, the URL and the element under the cursor
   are all recorded.
3. Quote tag: highlight text in any app as before a copy. A small pen
   chip appears below right of the release point for four seconds;
   click it, or press Ctrl+Shift+Q. The popover opens with the quote in
   a scrollable block above the comment box.
4. In the popover: severity, area and target chips. Ctrl+Up recalls the
   previous note and chips. Enter saves, Shift+Enter newline, Esc
   cancels. The note box spellchecks.
5. Ctrl+Shift+A right after a save: the next drag attaches a second crop
   to the tag just saved (comparison). Exported side by side.
6. Ctrl+Shift+R opens review: thumbnails, quote rows, drag to reorder,
   edit, drop. Carry forward tags from an earlier sweep; a carried tag
   has a Capture after button that takes one fresh crop and stores it
   beside the original. New sweep button with a name prompt.
7. Copy for chat puts the feedback sheet on the clipboard as Markdown.
   Copy as text does the same as plain text. Open opens any export in
   its default app. Save as opens a native save dialog. Export writes
   every file into the sweep folder and, when the sweep has tags for a
   target with an export directory configured, a copy of that target's
   evidence into it.

## Locked decisions

1. Gesture: pen chip AND Ctrl+Shift+Q.
2. Quote tags require the armed state.
3. Quote tags use the same severity and area chips as region tags.
4. No screenshot on quote tags by default; setting to attach the
   foreground window PNG.
5. The feedback sheet shares the sweep numbering and includes region
   tags as text plus a PNG pointer.
6. Save as uses tauri-plugin-dialog.

## Store: schema version 2

Tag fields. Every new field has a serde default so v1 files load
unchanged. `schemaVersion` is written as 2.

```
number, capturedUtc, monitorIndex, dpiScale, windowTitle, processName,
screenResolution, text, severity, area, dropped      unchanged

kind          "region" | "quote"           default "region"
image         Option<String>               crop; None for a quote tag unless quoteScreenshot is on
region        Option<Rect>                 None for quote tags
contextImage  Option<String>               tag-NN-context.png
quote         String                       default ""; the highlighted text
quoteHtml     Option<String>               raw CF_HTML fragment, stored, not rendered this round
url           String                       default ""; browser address bar when readable
element       String                       default ""; "button 'Deploy'" from UI Automation
target        String                       default ""; chip value
attachments   Vec<Attachment>              default empty
carriedFrom   Option<CarriedFrom>          default None

Attachment  { image, region: Rect, capturedUtc, label }   label "compare" | "after"
CarriedFrom { sweep, number, image: Option<String>, text }  image is the copied original crop
```

File names: `tag-NN.png` crop, `tag-NN-context.png` context frame,
`tag-NN-a1.png`, `tag-NN-a2.png` attachments, `tag-NN-before.png` the
copied original of a carried tag, `tag-NN-quote.png` the optional
window screenshot of a quote tag.

`image` stays present on region tags, so every existing test that reads
`tag.image` keeps its meaning after changing the type to Option.

## Settings additions (settings.json, all with defaults)

```
showPenChip      true
quoteScreenshot  false
quoteHotkey      "ctrl+shift+q"
attachHotkey     "ctrl+shift+a"
contextFrame     true
newSweepEachDay  false
targets          [ { name, hosts: [..], exportDir: null } ... ]
```

Default targets: helmsly (hosts: on.slobal.com, localhost, 127.0.0.1),
slobal.com (hosts: slobal.com, www.slobal.com), AgnCred (hosts:
agncred.com, www.agncred.com). Host match is suffix based on the URL
host, so a tenant subdomain of on.slobal.com maps to helmsly. The
settings window edits targets as rows: name, hosts (comma separated),
export directory.

## Reading the highlight (quote.rs)

1. Snapshot clipboard CF_UNICODETEXT and CF_HTML if present.
2. SendInput Ctrl+C to the foreground app. Modifier keys the operator is
   physically holding must be released in the injected stream first and
   not re-pressed afterwards.
3. Poll GetClipboardSequenceNumber up to 400 ms for a change; read
   CF_UNICODETEXT and CF_HTML (extract the fragment between
   StartFragment and EndFragment markers; a pure function with tests).
4. Restore the previous CF_UNICODETEXT (and CF_HTML if there was one).
   A previous clipboard holding an image or files is not restored;
   documented in the help window.
5. No change or empty text: toast "nothing highlighted", no tag.

Known edge, documented: in a terminal with nothing selected Ctrl+C is an
interrupt. The gesture is deliberate.

## Pen chip and hook changes (hook.rs, main.rs, overlay)

- While armed and no modifiers held, a plain left drag of 8 px or more,
  or a double or triple click, posts `Highlight(x, y)` on button up. The
  worker emits `highlight-hint` with CSS coordinates; the overlay shows
  a 28 px round chip with a pen glyph (inline SVG, no font, no asset)
  below right of the point. It fades after four seconds or on the next
  click elsewhere. Setting showPenChip off suppresses it.
- The hook holds the chip's screen rectangle in atomics. A left button
  down inside it is swallowed and posted as `QuoteClick`. The app
  underneath never sees the click, so its selection survives.
- Ctrl+Shift+Q (settings quoteHotkey) triggers the same quote path.
- Ctrl+Shift+A (settings attachHotkey) sets "attach to tag N" where N is
  the last saved tag in this armed session, then arms a one shot drag;
  the resulting crop becomes an attachment with label "compare" instead
  of a new tag. Toast "drag to attach to tag N". If no tag was saved in
  this session, toast "no tag to attach to".
- Capture after (review window) sets "attach to tag N" with label
  "after" for a carried tag, arms the app if needed, and one shots the
  next drag the same way. Review stays open; the toast names the tag.
- Selection start also records the URL and the element under the cursor
  (see below) before the crop is taken.

## Richer capture context (context.rs)

- URL: UI Automation on the foreground window. Chrome and Edge: the
  Edit control named "Address and search bar". Firefox: the Edit control
  whose name starts with "Search with" or contains "enter address".
  Read ValuePattern. Anything else: empty string. Never block more than
  250 ms; on timeout leave it empty and log.
- Element under cursor: IUIAutomation ElementFromPoint at the drag start
  point, rendered as `<control type> '<name>'`, for example
  `button 'Deploy'`. If the hit lands on TagFix's own overlay (process
  id equals ours), hide the overlay for one frame and retry once. Empty
  on failure.
- Context frame: after the crop, capture the foreground window rect
  (DwmGetWindowAttribute extended frame bounds, clamped to the monitor)
  through the existing capture path, scale so the longer side is at most
  1200 px, draw a 3 px red rectangle where the crop was, save as
  tag-NN-context.png. Setting contextFrame off skips it. Any failure
  logs and skips; the crop is never lost because of the frame.
- Target: from the URL host through the settings targets list, else "".

## Popover changes (index.html, main.js, style.css)

- Quote block above the note box for quote tags: monospace, six lines
  visible, scrolls, read only.
- Third chip row, target, populated from settings; preselected from the
  URL host; "other" always last.
- Ctrl+Up recalls the previous note text and chips (severity, area,
  target) from this process's last save. Ctrl+Up again does nothing
  further (single slot). Hint line mentions it.
- `spellcheck="true"` on the note textarea.

## Sweeps (store.rs, main.rs, tray, review)

- New sweep: tray item and review button open a small native style
  prompt window (Tauri window with one input) for a name; creates the
  sweep and makes it active. The review window sweep selector refreshes.
- Day rollover: with newSweepEachDay on, the first capture on a day with
  no sweep folder for that date creates `<date>-default` instead of
  extending yesterday's.
- Carry forward: review has a Carry forward button opening a picker of
  earlier sweeps and their tags (live and dropped). Chosen tags are
  copied into the current sweep with new numbers, `carriedFrom` set,
  the original crop copied to `tag-NN-before.png`, text and chips
  copied. The original sweep is untouched.

## Export (export.rs)

- fixlist.md and fixlist.html: quote tags render the quote as a
  blockquote where the image would be; region tags render the crop,
  then the context frame at reduced width, then any attachments side by
  side with their labels; carried tags render before (original) and
  after side by side with a line "Re-report of <sweep> tag NN". URL and
  element appear in the metadata line when present.
- brief.md: tasks grouped under a heading per target when any tag has a
  target; "Evidence: quote" for quote tags; carried tags say
  "Re-report; before and after images attached".
- feedback.md and feedback.txt: the feedback sheet.

```
Feedback on 2026-09-07-claude-round-98 (5 notes)
Sources: Claude - Helmsly round 98 (claude.exe); https://slobal.com/portal (msedge.exe)

1. [high / copy / helmsly]
> The Save button sits below the fold on a phone;
> the form needs a scroll before it can be sent.

Wrong: the button is pinned to the footer at every width.

2. [medium / behaviour / helmsly] (screenshot tag-02.png, button 'Deploy')

The Deploy button is enabled before a version is on the deployed list.
```

  The plain text variant drops the `>` and `[ ]` syntax for indented
  lines and a "Severity: high, area: copy, target: helmsly" line.
- Export copy into the target repo: for each target with an exportDir
  and at least one live tag, write `<exportDir>/<sweep dir name>/`
  containing fixlist.md, brief.md, feedback.md filtered to that target
  and every image those tags reference. Overwrite on re-export.
- Export still copies the brief pointer to the clipboard.

## Review window (review.html, review.js, review.css)

- Toolbar: `[sweep v] [New sweep] [Carry forward] [Export] [Copy for chat]
  [Copy as text] [Open v] [Save as v] status`.
- Rows: thumbnail of the crop (or the quote block for quote tags), click
  to enlarge in a lightbox; context frame and attachments as smaller
  thumbnails; carried rows show before and after and a Capture after
  button; metadata line adds URL and element; target select beside
  severity and area.
- Copy for chat and Copy as text render from sweep.json on the fly; no
  Export needed first.
- Open: fixlist.html, fixlist.md, brief.md, feedback.md, feedback.txt;
  renders first if missing or older than sweep.json; opens with the
  default app via ShellExecuteW.
- Save as: tauri-plugin-dialog save dialog with the file name prefilled,
  same five files, renders on the fly and writes to the chosen path.
  Capability `dialog:allow-save` only.
- Thumbnails are served through a `read_image` command returning base64;
  no asset protocol changes.

## Hotkeys after this round

| Keys | Meaning |
| --- | --- |
| Ctrl+Shift+T | arm / disarm (changeable) |
| Ctrl+Shift+drag | region tag |
| Ctrl+Shift+S | next plain drag is a region tag (trackpad) |
| Ctrl+Shift+Q | quote the current highlight (changeable) |
| Ctrl+Shift+A | next drag attaches a comparison crop to the last tag (changeable) |
| Ctrl+Shift+R | review and export |
| Ctrl+Up (popover) | recall the previous note and chips |

Every global registration failure warns at startup as today; the pen
chip and the review window keep working without their hotkeys.

## Phases

Each phase: implement, `cargo test --release` green, `cargo build
--release` clean of new warnings, launch the exe once and confirm
tagfix-startup.log reaches startup complete, kill it, commit `[BOT]
v0.3.0 phase N: <title>`, push.

- **Phase A, foundations**: schema v2 with every field above and its
  tests (v1 file loads, mixed sweep round trip, attachments, carried
  tags); settings additions with tests (defaults, partial file, targets
  host match); quote.rs (grab and restore, CF_HTML fragment tests, an
  example binary `examples/quote_check.rs` that prints what it grabbed);
  context.rs (URL, element, context frame; an example
  `examples/context_check.rs`); version 0.3.0 in Cargo.toml and
  tauri.conf.json.
- **Phase B, capture**: hook highlight and chip; overlay chip, quote
  popover, target chips, Ctrl+Up recall, spellcheck; Ctrl+Shift+Q;
  Ctrl+Shift+A attachments; context frame and URL and element wired into
  handle_selection_end; day rollover; New sweep from tray; settings
  window rows for every new setting including targets.
- **Phase C, export and review**: export.rs for every rendering above
  with tests (quote tags, attachments, carried tags, grouping by target,
  feedback sheet markdown and text, target export copies, no dashes);
  review window rebuild; Copy, Open, Save as; Carry forward; Capture
  after; tauri-plugin-dialog and its capability.
- **Phase D, docs and release**: help window, README (usage, hotkeys,
  data layout, the clipboard and terminal caveats), docs/smoke-test.md
  steps for every new behaviour, `dist/tagfix.exe` rebuilt, final
  commit, PR from `round/02-quote-tags` to main. No self merge.

## Tests

52 pass today. Target at the end of Phase C: at least 85, covering
schema v2 (load v1, round trip every new field), settings (targets host
suffix match, defaults), CF_HTML fragment extraction, chip hit test
geometry, context frame scaling and rectangle placement, feedback sheet
markdown and text, brief grouping by target, side by side rendering,
target export filtering, and the existing suites unchanged.

## Constraints

No em or en dashes anywhere (source, UI strings, docs, exports). No
telemetry, no network, no auto update. No tracker features: carried
tags and "after" captures are evidence, never a status field. Vanilla
JS, no bundler, no framework, no CDN, no web font. Commits `[BOT]`
prefixed with the trailer `Co-Authored-By: Claude <noreply@anthropic.com>`.
Git identity as configured in the checkout. Builds run through
the MSVC wrapper (vcvars64 plus .cargo/bin on PATH); a plain `cargo` in
a fresh shell does not link on this machine.

## Not in scope

Rich text rendering of quotes (the CF_HTML fragment is stored for a
later round), OCR, UI Automation based selection reading, any cloud or
sync.
