// TagFix settings window. Vanilla JS, no bundler, no framework.
const { invoke } = window.__TAURI__.core;

const hotkeyEl = document.getElementById("hotkey");
const quoteHotkeyEl = document.getElementById("quote-hotkey");
const attachHotkeyEl = document.getElementById("attach-hotkey");
const outputDirEl = document.getElementById("output-dir");
const launchLoginEl = document.getElementById("launch-login");
const showPenChipEl = document.getElementById("show-pen-chip");
const quoteScreenshotEl = document.getElementById("quote-screenshot");
const contextFrameEl = document.getElementById("context-frame");
const newSweepEachDayEl = document.getElementById("new-sweep-each-day");
const targetsBodyEl = document.getElementById("targets-body");
const statusEl = document.getElementById("status");

// Whatever was loaded, kept so fields this window does not edit (help
// already shown, and anything a later round adds) survive a save.
let loaded = {};

function addTargetRow(target) {
  const row = document.createElement("tr");

  const nameCell = document.createElement("td");
  const nameInput = document.createElement("input");
  nameInput.type = "text";
  nameInput.spellcheck = false;
  nameInput.className = "target-name";
  nameInput.value = target && target.name ? target.name : "";
  nameCell.appendChild(nameInput);

  const hostsCell = document.createElement("td");
  const hostsInput = document.createElement("input");
  hostsInput.type = "text";
  hostsInput.spellcheck = false;
  hostsInput.className = "target-hosts";
  hostsInput.value = target && target.hosts ? target.hosts.join(", ") : "";
  hostsCell.appendChild(hostsInput);

  const dirCell = document.createElement("td");
  const dirInput = document.createElement("input");
  dirInput.type = "text";
  dirInput.spellcheck = false;
  dirInput.className = "target-dir";
  dirInput.value = target && target.exportDir ? target.exportDir : "";
  dirCell.appendChild(dirInput);

  const removeCell = document.createElement("td");
  const removeBtn = document.createElement("button");
  removeBtn.className = "secondary remove";
  removeBtn.textContent = "Remove";
  removeBtn.addEventListener("click", () => row.remove());
  removeCell.appendChild(removeBtn);

  row.appendChild(nameCell);
  row.appendChild(hostsCell);
  row.appendChild(dirCell);
  row.appendChild(removeCell);
  targetsBodyEl.appendChild(row);
}

// A row with no name is a row the operator cleared out.
function readTargets() {
  const out = [];
  for (const row of targetsBodyEl.querySelectorAll("tr")) {
    const name = row.querySelector(".target-name").value.trim();
    if (name === "") continue;
    const hosts = row
      .querySelector(".target-hosts")
      .value.split(",")
      .map((h) => h.trim())
      .filter((h) => h !== "");
    const dir = row.querySelector(".target-dir").value.trim();
    out.push({ name, hosts, exportDir: dir === "" ? null : dir });
  }
  return out;
}

async function loadSettings() {
  const s = await invoke("get_settings");
  loaded = s;
  hotkeyEl.value = s.hotkey;
  quoteHotkeyEl.value = s.quoteHotkey || "ctrl+shift+q";
  attachHotkeyEl.value = s.attachHotkey || "ctrl+shift+a";
  outputDirEl.value = s.outputDir || "";
  launchLoginEl.checked = Boolean(s.launchAtLogin);
  showPenChipEl.checked = Boolean(s.showPenChip);
  quoteScreenshotEl.checked = Boolean(s.quoteScreenshot);
  contextFrameEl.checked = Boolean(s.contextFrame);
  newSweepEachDayEl.checked = Boolean(s.newSweepEachDay);
  targetsBodyEl.textContent = "";
  for (const target of s.targets || []) {
    addTargetRow(target);
  }
}

document.getElementById("add-target-btn").addEventListener("click", () => {
  addTargetRow(null);
});

document.getElementById("save-btn").addEventListener("click", async () => {
  const outputDir = outputDirEl.value.trim();
  const newSettings = Object.assign({}, loaded, {
    hotkey: hotkeyEl.value.trim() || "ctrl+shift+t",
    quoteHotkey: quoteHotkeyEl.value.trim() || "ctrl+shift+q",
    attachHotkey: attachHotkeyEl.value.trim() || "ctrl+shift+a",
    outputDir: outputDir === "" ? null : outputDir,
    launchAtLogin: launchLoginEl.checked,
    showPenChip: showPenChipEl.checked,
    quoteScreenshot: quoteScreenshotEl.checked,
    contextFrame: contextFrameEl.checked,
    newSweepEachDay: newSweepEachDayEl.checked,
    targets: readTargets(),
  });
  try {
    await invoke("save_settings", { newSettings });
    loaded = newSettings;
    statusEl.textContent = "saved";
  } catch (err) {
    statusEl.textContent = String(err);
  }
});

loadSettings();
