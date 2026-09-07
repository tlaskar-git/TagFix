// TagFix overlay UI. Vanilla JS, no bundler, no framework.
//
// While armed the overlay is click-through and silent: the operator keeps
// using the machine and sees nothing but a thin frame. The selection
// rectangle is driven by Ctrl+Shift+drag events coming from the Rust side
// global mouse hook. Only tag entry makes the overlay interactive.
//
// The pen chip is the one thing drawn while click-through that can be
// clicked: the hook hit tests it in screen pixels and swallows the click,
// so the highlight in the app underneath survives.
const { invoke } = window.__TAURI__.core;
const { listen } = window.__TAURI__.event;

const selectionEl = document.getElementById("selection");
const toastEl = document.getElementById("toast");
const popoverEl = document.getElementById("popover");
const tagTextEl = document.getElementById("tag-text");
const quoteBlockEl = document.getElementById("quote-block");
const targetRowEl = document.getElementById("target-chips");
const chipEl = document.getElementById("pen-chip");

// Kept in step with #pen-chip in style.css: the hook hit tests exactly
// this rectangle, so the number lives in one place on this side.
const CHIP_SIZE = 28;
// Below and right of the release point, clear of the cursor.
const CHIP_OFFSET = 14;
const CHIP_LIFETIME_MS = 4000;

let armed = false;
let entryOpen = false;
let toastTimer = null;
let chipTimer = null;

// The toast is the only thing that ever speaks, and only briefly.
// duration 0 keeps it up until something hides it.
function showToast(message, duration) {
  toastEl.textContent = message;
  document.body.classList.add("toast-visible");
  if (toastTimer) {
    clearTimeout(toastTimer);
    toastTimer = null;
  }
  if (duration > 0) {
    toastTimer = setTimeout(hideToast, duration);
  }
}

function hideToast() {
  if (toastTimer) {
    clearTimeout(toastTimer);
    toastTimer = null;
  }
  document.body.classList.remove("toast-visible");
}

function renderArmed(value) {
  armed = value;
  document.body.classList.toggle("armed", armed);
  document.body.classList.toggle("disarmed", !armed);
  if (armed) {
    // Deliberately silent: no banner while the operator works.
    invoke("overlay_ready");
  } else {
    document.body.classList.remove("one-shot");
    hideChip();
    closeEntry();
    hideSelection();
    hideToast();
  }
}

function hideSelection() {
  selectionEl.style.display = "none";
}

function drawSelection(r) {
  selectionEl.style.display = "block";
  selectionEl.style.left = r.x + "px";
  selectionEl.style.top = r.y + "px";
  selectionEl.style.width = r.w + "px";
  selectionEl.style.height = r.h + "px";
}

// The pen chip: shown below right of where a highlight ended, gone again
// after four seconds or on the next click elsewhere. The backend is told
// where it landed so the hook can take its click.
function showChip(point) {
  let x = point.x + CHIP_OFFSET;
  let y = point.y + CHIP_OFFSET;
  if (x + CHIP_SIZE > window.innerWidth - 4) {
    x = window.innerWidth - CHIP_SIZE - 4;
  }
  if (y + CHIP_SIZE > window.innerHeight - 4) {
    y = window.innerHeight - CHIP_SIZE - 4;
  }
  if (x < 4) x = 4;
  if (y < 4) y = 4;
  chipEl.style.left = x + "px";
  chipEl.style.top = y + "px";
  document.body.classList.add("chip-visible");
  if (chipTimer) clearTimeout(chipTimer);
  chipTimer = setTimeout(hideChip, CHIP_LIFETIME_MS);
  invoke("set_chip_rect", { x, y, w: CHIP_SIZE, h: CHIP_SIZE }).catch(() => {});
}

function hideChip() {
  if (chipTimer) {
    clearTimeout(chipTimer);
    chipTimer = null;
  }
  if (!document.body.classList.contains("chip-visible")) return;
  document.body.classList.remove("chip-visible");
  invoke("clear_chip_rect").catch(() => {});
}

// Chip rows: click selects, row remembers its value on data-value.
function wireChips(rowId) {
  const row = document.getElementById(rowId);
  row.addEventListener("click", (event) => {
    const btn = event.target.closest(".chip");
    if (!btn) return;
    for (const c of row.querySelectorAll(".chip")) {
      c.classList.toggle("selected", c === btn);
    }
    row.dataset.value = btn.dataset.value;
    tagTextEl.focus();
  });
}
wireChips("severity-chips");
wireChips("area-chips");
wireChips("target-chips");

function resetChips(rowId, def) {
  const row = document.getElementById(rowId);
  let hit = false;
  for (const c of row.querySelectorAll(".chip")) {
    const on = c.dataset.value === def;
    c.classList.toggle("selected", on);
    if (on) hit = true;
  }
  row.dataset.value = hit ? def : row.dataset.value;
}

// The target row is rebuilt per tag: the list comes from settings and
// "other" is always the last chip.
function buildTargetChips(names, selected) {
  const values = [];
  for (const name of names || []) {
    const clean = String(name).trim();
    if (clean && clean !== "other" && values.indexOf(clean) === -1) {
      values.push(clean);
    }
  }
  values.push("other");
  const chosen = selected && values.indexOf(selected) !== -1 ? selected : "other";
  targetRowEl.textContent = "";
  for (const value of values) {
    const btn = document.createElement("button");
    btn.className = value === chosen ? "chip selected" : "chip";
    btn.dataset.value = value;
    btn.textContent = value;
    targetRowEl.appendChild(btn);
  }
  targetRowEl.dataset.value = chosen;
}

function openEntry(payload) {
  entryOpen = true;
  hideChip();
  document.body.classList.add("entry");
  tagTextEl.value = "";
  resetChips("severity-chips", "medium");
  resetChips("area-chips", "other");
  buildTargetChips(payload.targets, payload.target);

  // A quote tag has words instead of pixels: show them above the note box,
  // read only, scrolling after six lines.
  const isQuote = payload.kind === "quote";
  document.body.classList.toggle("quote-entry", isQuote);
  quoteBlockEl.textContent = isQuote ? payload.quote || "" : "";
  quoteBlockEl.scrollTop = 0;

  // Keep the captured region outlined so it is clear what is being
  // described, and place the popover just below it.
  if (isQuote) {
    hideSelection();
  } else {
    drawSelection(payload);
  }

  const pw = 340;
  const ph = isQuote ? 320 : 200;
  let x = payload.x;
  let y = payload.y + payload.h + 10;
  if (x + pw > window.innerWidth) x = window.innerWidth - pw - 10;
  if (y + ph > window.innerHeight) y = payload.y - ph - 10;
  if (x < 10) x = 10;
  if (y < 10) y = 10;
  popoverEl.style.left = x + "px";
  popoverEl.style.top = y + "px";

  // Focus needs the window to have settled into interactive mode.
  setTimeout(() => tagTextEl.focus(), 30);
}

function closeEntry() {
  entryOpen = false;
  document.body.classList.remove("entry");
  document.body.classList.remove("quote-entry");
  hideSelection();
}

async function saveEntry() {
  const text = tagTextEl.value.trim();
  const severity = document.getElementById("severity-chips").dataset.value;
  const area = document.getElementById("area-chips").dataset.value;
  const target = targetRowEl.dataset.value === "other" ? "" : targetRowEl.dataset.value;
  closeEntry();
  try {
    const result = await invoke("save_tag", { text, severity, area, target });
    showToast("tag " + result.tagNumber + " saved", 1400);
  } catch (err) {
    showToast("save failed: " + err, 4000);
  }
}

async function cancelEntry() {
  closeEntry();
  try {
    await invoke("cancel_tag");
  } catch (err) {
    // Nothing pending; already cancelled.
  }
  hideToast();
}

// Ctrl+Up: the previous note and its chips, once. A single slot, so a
// second press changes nothing.
async function recallLastNote() {
  let last = null;
  try {
    last = await invoke("get_last_note");
  } catch (err) {
    last = null;
  }
  if (!last) return;
  tagTextEl.value = last.text || "";
  resetChips("severity-chips", last.severity || "medium");
  resetChips("area-chips", last.area || "other");
  buildTargetChips(
    Array.from(targetRowEl.querySelectorAll(".chip")).map((c) => c.dataset.value),
    last.target || "other"
  );
  tagTextEl.focus();
  tagTextEl.setSelectionRange(tagTextEl.value.length, tagTextEl.value.length);
}

tagTextEl.addEventListener("keydown", (event) => {
  if (event.key === "Enter" && !event.shiftKey) {
    event.preventDefault();
    saveEntry();
    return;
  }
  if (event.key === "ArrowUp" && event.ctrlKey) {
    event.preventDefault();
    recallLastNote();
  }
  // Shift+Enter falls through: the textarea inserts a newline itself.
});

window.addEventListener("keydown", (event) => {
  if (event.key === "Escape" && entryOpen) {
    event.preventDefault();
    cancelEntry();
  }
});

listen("armed-changed", (event) => {
  renderArmed(Boolean(event.payload));
});

// One-shot mode: the next plain drag marks a region, no chording needed.
// This one does need saying, because it changes what the next click does.
// The payload is a bare boolean, or an object when the drag is going to
// attach to a tag rather than start a new one.
listen("one-shot", (event) => {
  const p = event.payload;
  const isObject = p !== null && typeof p === "object";
  const on = isObject ? Boolean(p.on) : Boolean(p);
  document.body.classList.toggle("one-shot", on);
  if (on) {
    showToast(
      isObject && p.message ? p.message : "Drag now to mark a region",
      0
    );
  } else {
    hideToast();
  }
});

// Anything the backend wants said in passing: an attachment landed, or
// there was no tag to attach to.
listen("toast", (event) => {
  document.body.classList.remove("capturing");
  hideSelection();
  const p = event.payload || {};
  showToast(p.message || "", typeof p.duration === "number" ? p.duration : 2500);
});

// A highlight just ended: offer the pen chip beside it.
listen("highlight-hint", (event) => {
  if (entryOpen) return;
  showChip(event.payload);
});

listen("chip-dismiss", () => {
  hideChip();
});

listen("selection-start", () => {
  hideChip();
  hideSelection();
});

listen("selection-update", (event) => {
  if (!entryOpen) {
    drawSelection(event.payload);
  }
});

// Chrome off screen for the pixel grab.
listen("selection-hide", () => {
  document.body.classList.add("capturing");
  hideChip();
  hideSelection();
});

listen("selection-cancel", (event) => {
  document.body.classList.remove("capturing");
  hideSelection();
  // Never fail silently: say why nothing was captured, briefly.
  const why =
    event.payload && event.payload.reason
      ? event.payload.reason
      : "nothing captured";
  showToast(why, 2500);
});

listen("entry-open", (event) => {
  document.body.classList.remove("capturing");
  hideToast();
  openEntry(event.payload);
});

listen("entry-closed", () => {
  closeEntry();
});

invoke("ui_loaded");
invoke("get_armed").then(renderArmed);

// Backstop: events are the fast path, but if an IPC event never lands
// this keeps the overlay in step with the real armed state.
setInterval(() => {
  invoke("get_armed")
    .then((value) => {
      if (Boolean(value) !== armed) {
        renderArmed(Boolean(value));
      }
    })
    .catch(() => {});
}, 400);
