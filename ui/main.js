// TagFix overlay UI. Vanilla JS, no bundler, no framework.
const { invoke } = window.__TAURI__.core;
const { listen } = window.__TAURI__.event;

function renderArmed(armed) {
  document.body.classList.toggle("armed", armed);
  document.body.classList.toggle("disarmed", !armed);
}

listen("armed-changed", (event) => {
  renderArmed(Boolean(event.payload));
});

window.addEventListener("keydown", (event) => {
  if (event.key === "Escape") {
    invoke("set_armed", { armed: false });
  }
});

invoke("get_armed").then(renderArmed);
