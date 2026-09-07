// The new sweep prompt: one input, Enter creates, Esc closes. Closing runs
// through the backend so this window needs no window capability of its own.
const { invoke } = window.__TAURI__.core;

const nameEl = document.getElementById("sweep-name");
const statusEl = document.getElementById("status");

async function create() {
  const name = nameEl.value.trim();
  if (name === "") {
    statusEl.textContent = "give the sweep a name";
    return;
  }
  try {
    await invoke("create_sweep", { name });
    invoke("close_new_sweep");
  } catch (err) {
    statusEl.textContent = String(err);
  }
}

document.getElementById("create-btn").addEventListener("click", create);

window.addEventListener("keydown", (event) => {
  if (event.key === "Enter") {
    event.preventDefault();
    create();
  } else if (event.key === "Escape") {
    event.preventDefault();
    invoke("close_new_sweep");
  }
});

nameEl.focus();
