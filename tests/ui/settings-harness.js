// Same shim idea, aimed at ui/settings.js: the save payload must carry
// every field, including the ones this window does not edit.
const fs = require("fs");
const path = require("path");
const vm = require("vm");
// The repo root, two levels up from tests/ui, so the harness runs from
// anywhere and moves with a clone.
const REPO = path.resolve(__dirname, "..", "..");

class ClassList {
  constructor() { this.set = new Set(); }
  add(c) { this.set.add(c); } remove(c) { this.set.delete(c); }
  contains(c) { return this.set.has(c); }
  toggle(c, on) { if (on === undefined) on = !this.set.has(c); if (on) this.add(c); else this.remove(c); }
  toString() { return Array.from(this.set).join(" "); }
}
class El {
  constructor(tag, id) {
    this.tagName = tag; this.id = id || ""; this.children = []; this.parent = null;
    this.classList = new ClassList(); this.listeners = {}; this.value = ""; this._text = "";
    this.checked = false; this.dataset = {}; this.style = {};
  }
  set className(v) { this.classList = new ClassList(); for (const c of String(v).split(/\s+/)) if (c) this.classList.add(c); }
  get className() { return this.classList.toString(); }
  set textContent(v) { this._text = String(v); this.children = []; }
  get textContent() { return this._text; }
  appendChild(c) { c.parent = this; this.children.push(c); return c; }
  remove() { if (this.parent) this.parent.children = this.parent.children.filter((c) => c !== this); }
  addEventListener(n, fn) { (this.listeners[n] = this.listeners[n] || []).push(fn); }
  fire(n, e) { for (const fn of this.listeners[n] || []) fn(e || {}); }
  querySelectorAll(sel) {
    const want = sel.replace(".", ""); const out = [];
    const walk = (node) => { for (const c of node.children) {
      if (sel.startsWith(".") ? c.classList.contains(want) : c.tagName === sel) out.push(c);
      walk(c); } };
    walk(this); return out;
  }
  querySelector(sel) { return this.querySelectorAll(sel)[0] || null; }
}

const byId = {};
for (const id of ["hotkey", "quote-hotkey", "attach-hotkey", "output-dir", "launch-login",
  "show-pen-chip", "quote-screenshot", "context-frame", "new-sweep-each-day",
  "targets-body", "status", "add-target-btn", "save-btn"]) byId[id] = new El("div", id);

const stored = {
  hotkey: "ctrl+shift+t", outputDir: null, launchAtLogin: false,
  helpShown: true,
  showPenChip: true, quoteScreenshot: false, quoteHotkey: "ctrl+shift+q",
  attachHotkey: "ctrl+shift+a", contextFrame: true, newSweepEachDay: false,
  targets: [
    { name: "helmsly", hosts: ["on.slobal.com", "localhost"], exportDir: null },
    { name: "slobal.com", hosts: ["slobal.com"], exportDir: "D:\\sites" },
  ],
};

const calls = [];
const sandbox = {
  console, setTimeout, clearTimeout,
  document: { getElementById: (id) => byId[id] || null, createElement: (t) => new El(t) },
  window: { __TAURI__: { core: { invoke: (name, args) => {
    calls.push([name, args]);
    if (name === "get_settings") return Promise.resolve(JSON.parse(JSON.stringify(stored)));
    return Promise.resolve(null);
  } } } },
};
sandbox.globalThis = sandbox;
vm.createContext(sandbox);
vm.runInContext(fs.readFileSync(path.join(REPO, "ui/settings.js"), "utf8"), sandbox, { filename: "settings.js" });

let failures = 0;
function check(label, cond, extra) {
  if (cond) console.log("ok   " + label);
  else { failures++; console.log("FAIL " + label + (extra ? "  " + JSON.stringify(extra) : "")); }
}

setTimeout(() => {
  check("every field loads", byId["quote-hotkey"].value === "ctrl+shift+q" &&
    byId["attach-hotkey"].value === "ctrl+shift+a" && byId["show-pen-chip"].checked === true &&
    byId["context-frame"].checked === true && byId["quote-screenshot"].checked === false);
  const rows = byId["targets-body"].querySelectorAll("tr");
  check("a row per target", rows.length === 2, rows.length);
  check("hosts joined for editing", rows[0].querySelector(".target-hosts").value === "on.slobal.com, localhost",
    rows[0].querySelector(".target-hosts").value);
  check("export dir shown", rows[1].querySelector(".target-dir").value === "D:\\sites");

  // Add a row, edit it, and drop the first one.
  byId["add-target-btn"].fire("click");
  const added = byId["targets-body"].querySelectorAll("tr")[2];
  added.querySelector(".target-name").value = " AgnCred ";
  added.querySelector(".target-hosts").value = "agncred.com , www.agncred.com";
  added.querySelector(".target-dir").value = "  ";
  rows[0].querySelector(".remove").fire("click");

  byId["quote-screenshot"].checked = true;
  byId["new-sweep-each-day"].checked = true;
  byId["quote-hotkey"].value = "ctrl+alt+q";
  byId["save-btn"].fire("click");

  setTimeout(() => {
    const sent = calls.filter((c) => c[0] === "save_settings").pop();
    const s = sent && sent[1].newSettings;
    check("save sends a full settings object", s && s.hotkey === "ctrl+shift+t" &&
      s.quoteHotkey === "ctrl+alt+q" && s.attachHotkey === "ctrl+shift+a" &&
      s.quoteScreenshot === true && s.newSweepEachDay === true && s.contextFrame === true &&
      s.showPenChip === true, s);
    check("fields this window does not edit survive", s && s.helpShown === true);
    check("removed row is gone, added row is there",
      JSON.stringify(s.targets.map((t) => t.name)) === JSON.stringify(["slobal.com", "AgnCred"]),
      s && s.targets);
    check("hosts split and trimmed",
      JSON.stringify(s.targets[1].hosts) === JSON.stringify(["agncred.com", "www.agncred.com"]), s.targets[1].hosts);
    check("a blank export directory is null", s.targets[1].exportDir === null);
    check("status says saved", byId["status"].textContent === "saved");
    console.log(failures === 0 ? "\nALL SETTINGS CHECKS PASSED" : "\n" + failures + " SETTINGS CHECKS FAILED");
    process.exit(failures === 0 ? 0 : 1);
  }, 10);
}, 10);
