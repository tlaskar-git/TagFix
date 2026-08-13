// TagFix settings window. Vanilla JS, no bundler, no framework.
const { invoke } = window.__TAURI__.core;

const hotkeyEl = document.getElementById("hotkey");
const outputDirEl = document.getElementById("output-dir");
const launchLoginEl = document.getElementById("launch-login");
const statusEl = document.getElementById("status");

async function loadSettings() {
  const s = await invoke("get_settings");
  hotkeyEl.value = s.hotkey;
  outputDirEl.value = s.outputDir || "";
  launchLoginEl.checked = Boolean(s.launchAtLogin);
}

document.getElementById("save-btn").addEventListener("click", async () => {
  const outputDir = outputDirEl.value.trim();
  const newSettings = {
    hotkey: hotkeyEl.value.trim() || "ctrl+shift+t",
    outputDir: outputDir === "" ? null : outputDir,
    launchAtLogin: launchLoginEl.checked,
  };
  try {
    await invoke("save_settings", { newSettings });
    statusEl.textContent = "saved";
  } catch (err) {
    statusEl.textContent = String(err);
  }
});

loadSettings();
