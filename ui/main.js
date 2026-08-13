// TagFix overlay UI. Vanilla JS, no bundler, no framework.
const { invoke } = window.__TAURI__.core;
const { listen } = window.__TAURI__.event;

const selectionEl = document.getElementById("selection");
const counterEl = document.getElementById("tag-counter");
const popoverEl = document.getElementById("popover");
const tagTextEl = document.getElementById("tag-text");

let armed = false;
let entryOpen = false;
let dragging = false;
let startX = 0;
let startY = 0;

function renderArmed(value) {
  armed = value;
  document.body.classList.toggle("armed", armed);
  document.body.classList.toggle("disarmed", !armed);
  if (armed) {
    // Tell the watchdog the armed UI actually made it to the screen.
    requestAnimationFrame(() => invoke("overlay_ready"));
    refreshCounter();
  } else {
    closeEntry();
    cancelDrag();
  }
}

async function refreshCounter() {
  try {
    const s = await invoke("get_status");
    counterEl.textContent = " tag " + s.nextTagNumber + " in " + s.sweepName;
  } catch (err) {
    counterEl.textContent = "";
  }
}

function cancelDrag() {
  dragging = false;
  selectionEl.style.display = "none";
}

function selectionRect(evX, evY) {
  const x = Math.min(startX, evX);
  const y = Math.min(startY, evY);
  const w = Math.abs(evX - startX);
  const h = Math.abs(evY - startY);
  return { x, y, w, h };
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

  // Place the popover just under the captured region, clamped on screen.
  const pw = 340;
  const ph = 190;
  let x = region.x;
  let y = region.y + region.h + 8;
  if (x + pw > window.innerWidth) x = window.innerWidth - pw - 8;
  if (y + ph > window.innerHeight) y = region.y - ph - 8;
  if (x < 8) x = 8;
  if (y < 8) y = 8;
  popoverEl.style.left = x + "px";
  popoverEl.style.top = y + "px";
  tagTextEl.focus();
}

function closeEntry() {
  entryOpen = false;
  document.body.classList.remove("entry");
}

window.addEventListener("mousedown", (event) => {
  if (!armed || entryOpen || event.button !== 0) return;
  dragging = true;
  startX = event.clientX;
  startY = event.clientY;
  drawSelection(selectionRect(event.clientX, event.clientY));
});

window.addEventListener("mousemove", (event) => {
  if (!dragging) return;
  drawSelection(selectionRect(event.clientX, event.clientY));
});

window.addEventListener("mouseup", async (event) => {
  if (!dragging || event.button !== 0) return;
  dragging = false;
  const r = selectionRect(event.clientX, event.clientY);
  selectionEl.style.display = "none";
  if (r.w < 4 || r.h < 4) return;

  // Hide every piece of overlay chrome so the capture shows only the screen.
  document.body.classList.add("capturing");
  await new Promise((resolve) =>
    requestAnimationFrame(() => requestAnimationFrame(resolve))
  );

  try {
    const result = await invoke("capture_region", {
      x: r.x,
      y: r.y,
      w: r.w,
      h: r.h,
    });
    counterEl.textContent =
      " tag " + result.tagNumber + " in " + result.sweepName;
    document.body.classList.remove("capturing");
    openEntry(r);
  } catch (err) {
    document.body.classList.remove("capturing");
    counterEl.textContent = " capture failed: " + err;
  }
});

async function saveEntry() {
  const text = tagTextEl.value.trim();
  const severity = document.getElementById("severity-chips").dataset.value;
  const area = document.getElementById("area-chips").dataset.value;
  try {
    await invoke("save_tag", { text, severity, area });
  } catch (err) {
    counterEl.textContent = " save failed: " + err;
  }
  closeEntry();
  // Saving re-arms for the next tag: the overlay never disarmed, so just
  // refresh the counter.
  refreshCounter();
}

async function cancelEntry() {
  closeEntry();
  try {
    await invoke("cancel_tag");
  } catch (err) {
    // Already cancelled by the global Esc path; nothing to do.
  }
}

tagTextEl.addEventListener("keydown", (event) => {
  if (event.key === "Enter" && !event.shiftKey) {
    event.preventDefault();
    saveEntry();
  }
  // Shift+Enter falls through: the textarea inserts a newline itself.
});

window.addEventListener("keydown", (event) => {
  if (event.key !== "Escape") return;
  if (entryOpen) {
    cancelEntry();
  } else {
    invoke("set_armed", { armed: false });
  }
});

listen("armed-changed", (event) => {
  renderArmed(Boolean(event.payload));
});

listen("entry-cancelled", () => {
  // The global Esc shortcut already dropped the pending tag.
  closeEntry();
});

invoke("get_armed").then(renderArmed);
