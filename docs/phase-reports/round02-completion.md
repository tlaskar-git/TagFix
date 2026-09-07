# Round 02 completion report: TagFix v0.3.0

Date: 2026-09-07 (UTC)
Branch: `round/02-quote-tags`
Plan: docs/plans/round02-quote-tags.md (locked 2026-09-07)
Machine: Windows Server 2025, single RDP display 2496x1664 at 150 percent
scaling, no second monitor, no interactive desktop session.

## What landed, per phase

| Phase | Commit | What |
| --- | --- | --- |
| A foundations | `c539a63` | Schema version 2 (image and region become optional, nine new fields, Attachment and CarriedFrom, the file name helpers, carry_forward and sweep_exists_for_date); settings additions and the targets list with host matching; quote.rs (clipboard snapshot, injected Ctrl+C, restore, CF_HTML fragment extraction); context.rs (URL, element, context frame); version 0.3.0 |
| B capture | `a866444` | Hook highlight detection and the pen chip rectangle; overlay chip, quote popover, target chips, Ctrl+Up recall, spellcheck; Ctrl+Shift+Q; Ctrl+Shift+A attachments; URL, element, target and context frame wired into a region capture; day rollover; New sweep from the tray; settings window rows for everything new |
| C export and review | `7509ce9` | export.rs for quote tags, attachments, carried tags, target grouping and the feedback sheet in Markdown and text, plus the per target export copy; the review window rebuild (Copy for chat, Copy as text, Open, Save as, Carry forward, Capture after, lightbox); tauri-plugin-dialog and its single capability |
| D docs and release | this commit | help window, README, smoke test, the UI harness into the repo, dist/tagfix.exe rebuilt |

Tests: 52 at the start of the round, 123 at the end (113 in the library,
10 in the hook module, 0 doc tests). The plan asked for at least 85.
Growth was 52 to 91 in phase A, to 108 in phase B, to 123 in phase C.

Release build: clean, no new warnings. `dist/tagfix.exe` is 10,256,384
bytes (was about 9 MB in v0.2.3).

## Verified on this machine, and how

- **Rust suite**: `cargo test --release` through the MSVC wrapper. 123
  passed, 0 failed. This covers the schema (a v1 sweep.json loads
  unchanged, every version 2 field round trips, attachments, carried
  tags), settings defaults and host suffix matching, CF_HTML fragment
  extraction, the chip hit test and drag threshold geometry, context
  frame scaling and rectangle placement, every export rendering
  including the feedback sheet in both forms, brief grouping by target,
  and the target export filter.
- **UI harness**: `node tests/ui/run.js`. 74 checks across three
  harnesses (overlay 26, settings 10, review 38), all green. These run
  ui/main.js, ui/settings.js and ui/review.js under a hand written DOM
  shim in Node, so they prove wiring and rendering decisions, not pixels
  and not real input.
- **Startup**: `dist/tagfix.exe` launched once; `dist/tagfix-startup.log`
  reached `startup complete` through overlay webview created, hotkeys
  registered, mouse hook installed and tray built. Process then killed.
- **Diag**: `dist\tagfix.exe diag` wrote `TagFix diag 0.3.0`, WebView2
  152.0.4191.66, one monitor, `hotkey esc: free`.
- **Examples**: `examples/quote_check.rs` and `examples/context_check.rs`
  exist so the operator can drive the two Windows paths by hand. They
  were compiled, not run against a live selection: this session has no
  desktop to hold one.

## Needs the operator on a real desktop

Nothing in this round was ever driven with real input. This build machine
hides new tray icons and blocks synthetic input, so no TagFix window was
ever clicked, typed into or dragged on. Everything below is unexercised
end to end and is what docs/smoke-test.md's Round 02 sections are for:

- the pen chip: that it appears, where it appears, that it fades after
  four seconds, and above all that clicking it does not cost the app
  underneath its selection
- the live clipboard grab: that Ctrl+C reaches the foreground app with
  the operator's own modifiers released, that the poll sees the change,
  and that the previous text really comes back
- `Ctrl+Shift+Q` and `Ctrl+Shift+A` as registered global hotkeys, and the
  conflict message box when one of them is taken
- the attachment crop and Capture after: the one shot drag, the toast, and
  the file landing as `tag-NN-a1.png` or an `after` attachment
- the context frame as pixels: that the red rectangle lands where the crop
  actually came from on a 150 percent display
- URL and element reading from Chrome, Edge and Firefox, and the retry
  when the element hit lands on our own overlay
- day rollover, which needs a real date change
- the New sweep prompt window, from the tray and from review
- Open and Save as: ShellExecuteW opening the default app, and the native
  save dialog
- Carry forward against a real earlier sweep on disk
- the multi monitor and mixed DPI paths, as in round 01: this host has one
  display

## Decisions the phase agents made beyond the brief

From the three commit bodies and the code comments.

1. **An explicit active sweep marker, `sweeps/active.txt`** (phase B,
   store.rs). The brief left "the active sweep" implicit, which had meant
   "the folder name that sorts last". Once New sweep and the day rollover
   can both make a second sweep in one day, sorting last stops meaning
   most recent, so the active sweep is now written down.
2. **Host matching is longest pattern wins, at a dot boundary** (phase A,
   settings.rs). The brief said suffix based. Plain suffix matching files
   a tenant of on.slobal.com under slobal.com and matches notslobal.com
   against slobal.com. Both are wrong, so the rule is exact match or a
   match at a dot boundary, longest pattern first.
3. **`host_of_url` is deliberately not a URL parser** (phase A). Browsers
   hide the scheme, so `slobal.com/portal` has to work as well as a full
   URL with userinfo and a port. Anything unrecognisable yields an empty
   host and therefore no target, rather than a guess.
4. **`Tag::image_name()` and `Tag::region_or_zero()`** (phase A). Making
   `image` and `region` optional would otherwise have rippled through
   every renderer; these two keep the renderers on a `&str` and a `Rect`,
   so the round 01 export tests kept their meaning.
5. **The feedback sheet carries the tag number** (phase C). The brief's
   sample entry was `1. [high / copy / helmsly]`. The code writes
   `1. (tag 02) [high / copy / helmsly]`, because the reader of a pasted
   sheet otherwise has no way back to `tag-02.png`. The sheet's own
   numbering and the sweep's tag numbers diverge as soon as a tag is
   dropped.
6. **An empty target and the literal "other" read the same everywhere**
   (phase C). The popover writes "other" when nothing matched, the URL
   path writes "", and grouping, filtering and the chip line would
   otherwise treat them as two different targets.
7. **Untargeted tags group last, under "## Target: other"** (phase C).
   The brief said group by target when any tag has one, and said nothing
   about the leftovers.
8. **The one shot event payload became an object** (phase B, overlay).
   `one-shot` was a bare boolean for the trackpad mode; the attach flow
   needs to say which tag it is attaching to, so the payload is now
   either a boolean or `{ on, message }` and the overlay accepts both.
9. **`Ctrl+Shift+A` does nothing while disarmed** (phase B). The brief
   only specified the "no tag saved yet" case. Attaching without the
   overlay armed would have armed the hook behind the operator's back.
10. **The pen chip's button up is swallowed too, and a click anywhere
    else posts `ChipDismiss`** (phase B, hook.rs). Swallowing only the
    button down leaves a stray mouse up in the app underneath, which some
    apps read as a click.
11. **Open re-renders when the file is missing or older than sweep.json**
    (phase C). The brief said "renders first if missing or older"; the
    code made staleness the mtime comparison against sweep.json, so an
    edit in review is always reflected in what opens.
12. **The quote path runs off the main thread** (phase B). Grabbing a
    highlight waits on the clipboard for up to 400 ms and the mouse hook
    lives on the main thread; blocking it would freeze the desktop, which
    is the failure mode round 01 was built to avoid.
13. **The UI harness lives in `tests/ui/`, not `ui/`** (phase D).
    `tauri.conf.json` bundles everything under `ui/` into the exe, so a
    test file there would ship to users.

## Not done, and why

- No PR. The parent session opens it; ADR-049, no self merge.
- The CF_HTML fragment is stored and not rendered, as the plan scoped it.
- No tombstone, status or assignee field crept in: carried tags and after
  captures are evidence, per the round constraint.
