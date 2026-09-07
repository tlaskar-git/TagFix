// The shell of TagFix's one window: a sidebar and three sections. Section
// switching is client side; the hash names the section so a reload comes
// back where it was, and the backend opens the window on a hash or, when
// the window already exists, moves it with the show-section event.
//
// This file and each section's own script are wrapped in an IIFE, because
// on one page a second top level `const { invoke }` is a redeclaration
// error that would kill every script after it.
(() => {
  const { invoke } = window.__TAURI__.core;
  const { listen } = window.__TAURI__.event;

  const SECTIONS = ["review", "settings", "help"];
  const DEFAULT_SECTION = "review";

  function nameOf(value) {
    const name = String(value === undefined || value === null ? "" : value)
      .replace(/^#/, "")
      .trim();
    return SECTIONS.indexOf(name) >= 0 ? name : DEFAULT_SECTION;
  }

  function show(value) {
    const wanted = nameOf(value);
    for (const section of SECTIONS) {
      const panel = document.getElementById("section-" + section);
      const nav = document.getElementById("nav-" + section);
      if (panel) panel.classList.toggle("active", section === wanted);
      if (nav) nav.classList.toggle("active", section === wanted);
    }
    // The hash is what a reload reads, so it follows the sidebar.
    if (window.location.hash !== "#" + wanted) {
      window.location.hash = "#" + wanted;
    }
    return wanted;
  }

  for (const section of SECTIONS) {
    const nav = document.getElementById("nav-" + section);
    if (nav) nav.addEventListener("click", () => show(section));
  }

  // Sent when a tray item or a hotkey asks for a section and the window is
  // already open. A window being created gets the section in its hash.
  listen("show-section", (event) => show(event.payload));

  window.addEventListener("hashchange", () => show(window.location.hash));

  // One line in the runtime log saying the page booted and where it opened,
  // so a window that never paints can be told from one that never loaded.
  invoke("main_window_ready", { section: show(window.location.hash) });
})();
