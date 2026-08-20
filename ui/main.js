// TagFix overlay UI. Vanilla JS, no bundler, no framework.
//
// While armed the overlay is click-through and silent: the operator keeps
// using the machine and sees nothing but a thin frame. The selection
// rectangle is driven by Ctrl+Shift+drag events coming from the Rust side
// global mouse hook. Only tag entry makes the overlay interactive.
const { invoke } = window.__TAURI__.core;
const { listen } = window.__TAURI__.event;

const selectionEl = document.getElementById("selection");
const toastEl = document.getElementById("toast");
const popoverEl = document.getElementById("popover");
const tagTextEl = document.getElementById("tag-text");

let armed = false;
let entryOpen = false;
let toastTimer = null;

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

function resetChips(rowId, def) {
  const row = document.getElementById(rowId);
  for (const c of row.querySelectorAll(".chip")) {
    c.classList.toggle("selected", c.dataset.value === def);
  }
  row.dataset.value = def;
}

function openEntry(region) {
  entryOpen = true;
  document.body.classList.add("entry");
  tagTextEl.value = "";
  resetChips("severity-chips", "medium");
  resetChips("area-chips", "other");

  // Keep the captured region outlined so it is clear what is being
  // described, and place the popover just below it.
  drawSelection(region);

  const pw = 340;
  const ph = 200;
  let x = region.x;
  let y = region.y + region.h + 10;
  if (x + pw > window.innerWidth) x = window.innerWidth - pw - 10;
  if (y + ph > window.innerHeight) y = region.y - ph - 10;
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
  hideSelection();
}

async function saveEntry() {
  const text = tagTextEl.value.trim();
  const severity = document.getElementById("severity-chips").dataset.value;
  const area = document.getElementById("area-chips").dataset.value;
  closeEntry();
  try {
    const result = await invoke("save_tag", { text, severity, area });
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

tagTextEl.addEventListener("keydown", (event) => {
  if (event.key === "Enter" && !event.shiftKey) {
    event.preventDefault();
    saveEntry();
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
listen("one-shot", (event) => {
  const on = Boolean(event.payload);
  document.body.classList.toggle("one-shot", on);
  if (on) {
    showToast("Drag now to mark a region", 0);
  } else {
    hideToast();
  }
});

listen("selection-start", () => {
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
