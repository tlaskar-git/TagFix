// TagFix review window. Vanilla JS, no bundler, no framework.
const { invoke } = window.__TAURI__.core;

const sweepSelect = document.getElementById("sweep-select");
const tagList = document.getElementById("tag-list");
const exportBtn = document.getElementById("export-btn");
const statusEl = document.getElementById("status");

let currentSweep = null;
let dragRow = null;

function setStatus(msg) {
  statusEl.textContent = msg;
}

async function loadSweepList() {
  const sweeps = await invoke("list_sweeps");
  sweepSelect.innerHTML = "";
  for (const [name, count] of sweeps) {
    const opt = document.createElement("option");
    opt.value = name;
    opt.textContent = name + " (" + count + " tags)";
    sweepSelect.appendChild(opt);
  }
  if (sweeps.length > 0) {
    currentSweep = sweeps[0][0];
    sweepSelect.value = currentSweep;
    await loadTags();
  } else {
    setStatus("no sweeps yet");
  }
}

function severitySelect(tag) {
  const sel = document.createElement("select");
  for (const s of ["high", "medium", "low"]) {
    const opt = document.createElement("option");
    opt.value = s;
    opt.textContent = s;
    if (tag.severity === s) opt.selected = true;
    sel.appendChild(opt);
  }
  sel.addEventListener("change", () =>
    saveTagEdit(tag, { severity: sel.value })
  );
  return sel;
}

function areaSelect(tag) {
  const sel = document.createElement("select");
  for (const a of ["layout", "copy", "a11y", "behaviour", "other"]) {
    const opt = document.createElement("option");
    opt.value = a;
    opt.textContent = a;
    if (tag.area === a) opt.selected = true;
    sel.appendChild(opt);
  }
  sel.addEventListener("change", () => saveTagEdit(tag, { area: sel.value }));
  return sel;
}

async function saveTagEdit(tag, changes) {
  const text = "text" in changes ? changes.text : tag.text;
  const severity = "severity" in changes ? changes.severity : tag.severity;
  const area = "area" in changes ? changes.area : tag.area;
  try {
    await invoke("update_tag", {
      dirName: currentSweep,
      number: tag.number,
      text,
      severity,
      area,
    });
    tag.text = text;
    tag.severity = severity;
    tag.area = area;
    setStatus("saved tag " + tag.number);
  } catch (err) {
    setStatus("save failed: " + err);
  }
}

function buildRow(tag) {
  const li = document.createElement("li");
  li.className = "tag-row" + (tag.dropped ? " dropped-row" : "");
  li.draggable = true;
  li.dataset.number = tag.number;

  const handle = document.createElement("span");
  handle.className = "drag-handle";
  handle.textContent = "::";
  li.appendChild(handle);

  const num = document.createElement("span");
  num.className = "tag-number";
  num.textContent = "tag " + String(tag.number).padStart(2, "0");
  li.appendChild(num);

  const body = document.createElement("div");
  body.className = "tag-body";

  const textEl = document.createElement("div");
  textEl.className = "tag-text" + (tag.text ? "" : " empty");
  textEl.textContent = tag.text || "no text";
  textEl.addEventListener("click", () => {
    const ta = document.createElement("textarea");
    ta.rows = 3;
    ta.value = tag.text;
    body.replaceChild(ta, textEl);
    ta.focus();
    const done = async () => {
      await saveTagEdit(tag, { text: ta.value.trim() });
      renderTags();
    };
    ta.addEventListener("blur", done);
    ta.addEventListener("keydown", (ev) => {
      if (ev.key === "Enter" && !ev.shiftKey) {
        ev.preventDefault();
        ta.blur();
      }
    });
  });
  body.appendChild(textEl);

  const meta = document.createElement("div");
  meta.className = "tag-meta";
  meta.textContent =
    tag.image +
    "  |  " +
    tag.capturedUtc +
    "  |  " +
    tag.windowTitle +
    " (" +
    tag.processName +
    ")";
  body.appendChild(meta);
  li.appendChild(body);

  const controls = document.createElement("div");
  controls.className = "row-controls";
  controls.appendChild(severitySelect(tag));
  controls.appendChild(areaSelect(tag));

  const dropBtn = document.createElement("button");
  dropBtn.className = "drop-btn";
  dropBtn.textContent = tag.dropped ? "pick back up" : "drop";
  dropBtn.addEventListener("click", async () => {
    try {
      await invoke("set_dropped", {
        dirName: currentSweep,
        number: tag.number,
        dropped: !tag.dropped,
      });
      tag.dropped = !tag.dropped;
      renderTags();
    } catch (err) {
      setStatus("drop failed: " + err);
    }
  });
  controls.appendChild(dropBtn);
  li.appendChild(controls);

  li.addEventListener("dragstart", () => {
    dragRow = li;
    li.classList.add("dragging");
  });
  li.addEventListener("dragend", async () => {
    li.classList.remove("dragging");
    dragRow = null;
    const order = [...tagList.querySelectorAll(".tag-row")].map((r) =>
      parseInt(r.dataset.number, 10)
    );
    try {
      await invoke("reorder_tags", { dirName: currentSweep, order });
      setStatus("order saved");
    } catch (err) {
      setStatus("reorder failed: " + err);
    }
  });
  li.addEventListener("dragover", (ev) => {
    ev.preventDefault();
    if (!dragRow || dragRow === li) return;
    const rect = li.getBoundingClientRect();
    const before = ev.clientY < rect.top + rect.height / 2;
    tagList.insertBefore(dragRow, before ? li : li.nextSibling);
  });

  return li;
}

let tags = [];

function renderTags() {
  tagList.innerHTML = "";
  for (const tag of tags) {
    tagList.appendChild(buildRow(tag));
  }
}

async function loadTags() {
  const sweep = await invoke("load_sweep", { dirName: currentSweep });
  tags = sweep.tags;
  renderTags();
  setStatus(tags.length + " tags loaded");
}

sweepSelect.addEventListener("change", async () => {
  currentSweep = sweepSelect.value;
  await loadTags();
});

exportBtn.addEventListener("click", async () => {
  if (!currentSweep) return;
  try {
    const pointer = await invoke("export_sweep", { dirName: currentSweep });
    let copied = false;
    try {
      await navigator.clipboard.writeText(pointer);
      copied = true;
    } catch (clipErr) {
      copied = false;
    }
    setStatus(
      "exported fixlist.md, fixlist.html, brief.md" +
        (copied ? "; brief pointer copied to clipboard" : "")
    );
  } catch (err) {
    setStatus("export failed: " + err);
  }
});

loadSweepList();
