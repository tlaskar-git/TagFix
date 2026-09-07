# TagFix manual smoke test

Run these steps on Windows 11 x64 after each phase lands. Steps accumulate as
phases are delivered.

## Phase 1: shell

1. Launch `tagfix.exe`. Expect: no visible window, tray icon appears (drag it
   out of the tray overflow on first run if Windows hides it).
2. Right-click the tray icon. Expect menu: Arm, Review and export, Open
   sweeps folder, Settings, Quit.
3. Press `Ctrl+Shift+T`. Expect: a thin red frame appears at the screen
   edge, and nothing else: no banner, no message.
4. Press `Ctrl+Shift+T` again. Expect: the frame disappears.
5. While disarmed, click on a window underneath the overlay. Expect: the
   click lands on that window, not on the overlay.
6. Tray menu, Open sweeps folder. Expect: Explorer opens the sweeps
   directory.
7. Tray menu, Quit. Expect: process exits, tray icon disappears.

## Phase 2: capture

1. Arm. Expect: only the thin frame appears, no banner. Click
   around, type in an app, scroll: everything must behave normally,
   because the overlay is click-through in standby.
2. Hold `Ctrl+Shift` and drag a box over a window. Expect: red selection
   rectangle follows the drag, and the app underneath does not receive
   the click.
2b. Trackpad path: press `Ctrl+Shift+S`. Expect: a DRAG NOW prompt
   appears. Drag with no keys held. Expect: the same selection
   behaviour, the prompt clears, and the mode ends after that one drag.
2c. If a click never starts a selection, check tagfix-runtime.log for
   "click seen while armed but not a capture gesture (ctrl=..,
   shift=..)". That line proves the hook sees the click and shows what
   the keyboard state looked like at that moment.
3. Release. Expect: a popover opens; a `tag-NN.png` appears in the active
   sweep folder containing exactly the dragged region, with no TagFix
   chrome (no red frame, prompt, or selection box) in the pixels.
4. On a machine with a secondary monitor at 150 percent scaling: move the
   cursor to that monitor, arm, drag a region. Expect: pixel-correct PNG and
   a region rect in sweep.json matching the physical pixels.
5. Check sweep.json. Expect per tag: capturedUtc, monitorIndex, dpiScale,
   region, windowTitle, processName, screenResolution.

## Phase 3: tag entry

1. Capture a region. Expect: popover near the region with text field,
   severity chips (high, medium, low) and area chips (layout, copy, a11y,
   behaviour, other), medium and other preselected.
2. Type a note, press `Enter`. Expect: popover closes, a brief "tag N
   saved" confirmation appears and clears itself, and the machine is
   immediately usable again (click something underneath to confirm)
   while still armed.
3. Capture another region, press `Shift+Enter` inside the text field.
   Expect: newline, no save.
4. Press `Esc` with the popover open. Expect: popover closes, no tag saved,
   the pending PNG is deleted, back to usable standby.
5. Speed check: five tags in under sixty seconds using only Ctrl+Shift
   drags and typing.
6. Press `Ctrl+Shift+T` to disarm, then `Ctrl+Shift+drag`. Expect: nothing
   happens and the drag reaches the app underneath.
7. While armed but not tagging, press `Esc` in another app. Expect: that
   app receives it; TagFix does not swallow Esc.

## Phase 4: sweep store

1. `tagfix sweep new my-slug` then `tagfix sweep list`. Expect: the new
   sweep listed with 0 tags.
2. Capture two tags, kill the process from Task Manager mid-typing on a
   third. Relaunch. Expect: the first two tags intact, only the in-flight
   tag lost.

## Phase 5: review and export

1. `Ctrl+Shift+R` (or tray menu, Review and export). Expect: tags listed in
   order.
2. Drag a row to a new position, close and reopen the window. Expect: order
   kept.
3. Edit text, change severity, drop a tag. Expect: changes persist;
   the dropped tag stays in sweep.json with `"dropped": true`.
4. Press Export. Expect: fixlist.md, fixlist.html, brief.md in the sweep
   folder and a pointer to brief.md on the clipboard.
5. Open fixlist.html in a browser with the network cable pulled (or devtools
   offline). Expect: every image visible, zero external requests.

## Regression: arm must never wedge the desktop

The v0.1.0 to v0.1.6 freeze was a deadlock: the Esc global shortcut was
registered from inside the global shortcut handler's own thread, which
wedged that thread and the main thread with the full screen overlay
already visible. It only reproduced where Esc was genuinely unregistered,
because an existing registration returns an error immediately instead of
doing the real work.

1. Confirm Esc is free (`tagfix diag` reports `hotkey esc: free`).
2. Arm, disarm, and re-arm several times in a row.
3. Expect: the app stays responsive every time, and tagfix-runtime.log
   shows `apply_armed: returning` followed by the esc and emit lines from
   their own threads.
4. Expect: no arm ever leaves the overlay on screen for more than six
   seconds without the watchdog hiding it.

## Phase 6: settings and packaging

1. Tray menu, Settings. Change the hotkey to `ctrl+alt+f9`, save. Expect:
   old hotkey dead, new hotkey arms.
2. Set an output directory, save, capture a tag. Expect: sweep lands there.
3. Toggle launch at login, save. Expect: `TagFix` value appears under
   HKCU\Software\Microsoft\Windows\CurrentVersion\Run, and disappears when
   toggled off.
4. Copy `tagfix.exe` alone to a fresh folder (no repo, no target dir) and
   run it as a non-admin user. Expect: it runs, captures, and exports.

## Round 02 (v0.3.0): quote tags

Arm first (`Ctrl+Shift+T`) for every step here: quote tags need the armed
state, exactly like region tags.

1. Open a page of text (a browser, a chat window, Notepad). Highlight a
   sentence with the mouse and let go. Expect: a small round pen chip
   appears below right of where you released, and fades on its own after
   about four seconds. Clicking anywhere else dismisses it too. The chip
   itself is the evidence; nothing is written yet.
2. Highlight again and click the pen chip. Expect: the popover opens with
   the quote in a read only monospace block above the note box, and the
   text is still highlighted in the app underneath, because the chip
   click never reached it. Proof: tagfix-runtime.log shows `chip: clicked`
   then `quote: grabbing the highlight` then
   `quote: entry open for tag NN`.
3. Type a note and press Enter. Expect: the tag saves. Proof: sweep.json
   in the active sweep folder has a tag with `"kind": "quote"`, the
   highlighted sentence in `"quote"`, `"image": null` and
   `"region": null`.
4. Highlight a sentence and press `Ctrl+Shift+Q` instead of clicking the
   chip. Expect: the same popover. Proof: the same
   `quote: grabbing the highlight` and `quote: entry open for tag NN`
   lines, with no `chip: clicked` before them.
5. Nothing highlighted: click once in a text area to clear the selection,
   then press `Ctrl+Shift+Q`. Expect: the toast "nothing highlighted" and
   no popover. Proof: tagfix-runtime.log line
   `quote: nothing highlighted`, and sweep.json gains no tag.
6. Clipboard restored: copy the word `sentinel` in Notepad, then highlight
   something else and quote it. Save or cancel, then paste. Expect:
   `sentinel` pastes back. The paste is the proof. Now copy an image or a
   file in Explorer instead and repeat: that clipboard is gone
   afterwards. That is the documented limit, not a bug.
7. `Ctrl+Up` recall: save a tag with a note and non default chips, then
   open a second tag and press `Ctrl+Up` in the note box. Expect: the
   previous note text and the same severity, area and target chips come
   back. Press `Ctrl+Up` again: nothing further changes, because it is a
   single slot.
8. Spellcheck: type a misspelled word in the note box. Expect: the red
   squiggle. Proof: `spellcheck="true"` on the textarea in
   ui/index.html.
9. Target from a URL: open a slobal.com page in Chrome, Edge or Firefox,
   highlight some text and quote it. Expect: the target chip row shows
   helmsly, slobal.com, AgnCred and other, with slobal.com already
   selected. Proof: sweep.json has that address in `"url"` and
   `"target": "slobal.com"`. Repeat on a tenant page under on.slobal.com:
   expect helmsly, not slobal.com, because the longer host pattern wins.

## Round 02 (v0.3.0): richer region capture

1. Context frame: with Save a context frame on in Settings (the default),
   Ctrl+Shift+drag a small box inside a large window. Expect `tag-NN.png`
   (the crop) and `tag-NN-context.png` beside it. Open the context frame:
   the whole window shrunk to at most 1200 px on its longer side, a 3 px
   red rectangle exactly where the crop came from, and no TagFix chrome.
   Proof: sweep.json has `"contextImage": "tag-NN-context.png"`. If the
   frame is skipped, tagfix-runtime.log says `context frame: skipped
   (...)` and the crop is still there, which is the intended trade.
2. Turn Save a context frame off in Settings and capture again. Expect: no
   `tag-NN-context.png`, and `"contextImage": null` in sweep.json.
3. URL and element: in Chrome, Edge or Firefox, drag a box that starts on
   a named button. Expect in sweep.json: `"url"` holding the address bar
   text and `"element"` reading like `button 'Deploy'`. Outside those
   three browsers `"url"` is an empty string, by design. If the element
   hit lands on TagFix's own overlay, tagfix-runtime.log says
   `element: hit our own overlay, retrying with it hidden`.
4. Attachment: save a tag, then press `Ctrl+Shift+A`. Expect: the toast
   "drag to attach to tag N". Drag a second box. Expect: the toast
   "attached to tag N" and no new popover. Proof: `tag-NN-a1.png` on
   disk, an entry in that tag's `"attachments"` with
   `"label": "compare"`, and tagfix-runtime.log lines
   `one shot: next drag attaches to tag NN` then `capture: done via ...`.
   Repeat: the next one is `tag-NN-a2.png`.
5. Nothing to attach to: restart TagFix, arm, and press `Ctrl+Shift+A`
   before saving anything. Expect: the toast "no tag to attach to", and
   the next drag does nothing.

## Round 02 (v0.3.0): sweeps

1. New sweep from the tray: tray icon, New sweep. Expect a small window
   with a name field. Type `round 99 login` and press Enter. Expect: the
   window closes and the next capture lands in
   `sweeps/<today>-round-99-login/`. Proof: tagfix-runtime.log line
   `sweep: created <date>-round-99-login`, and `sweeps/active.txt`
   holding that folder name.
2. New sweep from review: `Ctrl+Shift+R`, New sweep, the same prompt.
   Expect: the same result, and the review window's sweep selector
   refreshes and selects the new sweep without being reopened.
3. Day rollover: turn Start a new sweep on the first capture of each day
   on in Settings. Move the machine clock forward one day (or come back
   tomorrow) and capture a tag. Expect: a new `<date>-default` folder
   holding that tag, and yesterday's sweep untouched. Proof:
   tagfix-runtime.log line `sweep: day rollover, starting <date>-default`.
4. Carry forward: in review, on the current sweep, press Carry forward and
   choose an earlier sweep. Expect: its tags listed with their first line
   and a thumbnail, dropped ones marked. Tick two and press Carry.
   Expect: two new rows at the end of the current sweep, each headed as a
   re-report. Proof: sweep.json has those tags with a `"carriedFrom"`
   object naming the source sweep and number, and `tag-NN-before.png`
   files holding the copied original crops. Open the source sweep: it is
   unchanged, same numbers, same files.
5. Capture after: on a carried row press Capture after. Expect: the toast
   "drag to attach to tag N", the review window stays open, and TagFix
   arms itself if it was not armed. Drag a box over the fixed version.
   Expect: an attachment with `"label": "after"` on that tag, and the row
   showing before and after thumbnails side by side.

## Round 02 (v0.3.0): review and export

1. Thumbnails: `Ctrl+Shift+R` on a sweep holding a region tag, a quote tag
   and a carried tag. Expect: the region row shows the crop plus the
   context frame plus any attachments as smaller labelled thumbnails; the
   quote row shows the quote text in a block instead of a picture; the
   carried row shows before and after.
2. Lightbox: click any thumbnail. Expect: it fills the window. Press Esc.
   Expect: it closes and the review list is back.
3. Copy for chat: press it on a sweep that has never been exported.
   Expect: the status says the sheet is on the clipboard. Paste into a
   chat window. Expect: the header line
   `Feedback on <sweep dir name> (N notes)`, a `Sources:` line, then
   numbered entries each carrying `(tag NN)`, its
   `[severity / area / target]` chips, quotes as `>` blockquotes and the
   note underneath. Proof: it pasted, and the sweep folder still has no
   feedback.md, because nothing was exported.
4. Copy as text: same sweep, press it, paste. Expect: the same content
   with no `>` and no `[ ]`: indented quote lines and a spelled out
   severity, area and target line.
5. Open: press Open and pick each of fixlist.md, fixlist.html, brief.md,
   feedback.md and feedback.txt in turn. Expect: each opens in its default
   app. Proof: the file now exists in the sweep folder with a timestamp
   newer than sweep.json, because Open renders it first when it is
   missing or stale.
6. Save as: press Save as and pick fixlist.html, then choose a folder.
   Expect: a native save dialog with the file name prefilled, and the file
   written where you chose. It does not need to exist in the sweep folder
   first.
7. Target export folder: in Settings give the slobal.com target an export
   directory that exists, save, then export a sweep holding at least one
   slobal.com tag. Expect: `<that directory>/<sweep dir name>/` holding
   fixlist.md, brief.md and feedback.md filtered to slobal.com tags only,
   plus every image those tags reference. Proof: the review status names
   the folder it wrote, and a helmsly tag from the same sweep does not
   appear in that copy. Export again: the folder is overwritten, not
   duplicated.
8. Export with no target directories set: press Export. Expect: five files
   in the sweep folder (fixlist.md, fixlist.html, brief.md, feedback.md,
   feedback.txt) and the brief.md pointer on the clipboard, as before, and
   no other folder written.

## Round 02 (v0.3.0): the documented rough edges

1. Hotkey conflict: set the quote hotkey in Settings to a combination
   another program already owns, and restart TagFix. Expect: a message
   box titled "TagFix hotkey conflict" naming the combination and saying
   the pen chip after a highlight still works. Dismiss it. Expect: TagFix
   is running, the tray icon is there, arming works, and the pen chip
   opens a quote tag. Proof: tagfix-startup.log still reaches
   `startup complete`. A conflict must never be fatal. (`tagfix diag`
   probes only ctrl+shift+t, ctrl+shift+r and esc, so it will not report
   this one; the message box is the evidence.)
2. Terminal Ctrl+C: open a terminal, start a long running command, and
   with nothing selected in the window press `Ctrl+Shift+Q`. Expect: the
   command is interrupted, because Ctrl+C with nothing selected is an
   interrupt in a terminal, and TagFix says "nothing highlighted". That is
   the documented cost of the gesture. Highlight text in the terminal
   first and it quotes cleanly instead.
