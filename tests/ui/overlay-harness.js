// Minimal DOM shim so ui/main.js can be exercised in Node. Not a browser:
// only what main.js actually touches is implemented.
const fs = require("fs");
const path = require("path");
const vm = require("vm");

// The repo root, two levels up from tests/ui, so the harness runs from
// anywhere and moves with a clone.
const REPO = path.resolve(__dirname, "..", "..");

class ClassList {
  constructor() { this.set = new Set(); }
  add(c) { this.set.add(c); }
  remove(c) { this.set.delete(c); }
  contains(c) { return this.set.has(c); }
  toggle(c, on) { if (on === undefined) on = !this.set.has(c); if (on) this.add(c); else this.remove(c); return on; }
  toString() { return Array.from(this.set).join(" "); }
}

class El {
  constructor(tag, id) {
    this.tagName = tag;
    this.id = id || "";
    this.children = [];
    this.parent = null;
    this.style = {};
    this.dataset = {};
    this.classList = new ClassList();
    this.listeners = {};
    this.value = "";
    this._text = "";
    this.spellcheck = undefined;
  }
  set className(v) {
    this.classList = new ClassList();
    for (const c of String(v).split(/\s+/)) if (c) this.classList.add(c);
  }
  get className() { return this.classList.toString(); }
  set textContent(v) { this._text = String(v); this.children = []; }
  get textContent() { return this._text; }
  appendChild(child) { child.parent = this; this.children.push(child); return child; }
  remove() { if (this.parent) this.parent.children = this.parent.children.filter((c) => c !== this); }
  addEventListener(name, fn) { (this.listeners[name] = this.listeners[name] || []).push(fn); }
  fire(name, event) { for (const fn of this.listeners[name] || []) fn(event); }
  querySelectorAll(sel) {
    const want = sel.replace(".", "");
    const out = [];
    const walk = (node) => {
      for (const c of node.children) {
        if (sel.startsWith(".") ? c.classList.contains(want) : c.tagName === sel) out.push(c);
        walk(c);
      }
    };
    walk(this);
    return out;
  }
  closest(sel) {
    const want = sel.replace(".", "");
    let node = this;
    while (node) {
      if (node.classList.contains(want)) return node;
      node = node.parent;
    }
    return null;
  }
  focus() { this.focused = true; }
  setSelectionRange() {}
}

const byId = {};
function make(id, tag) { const e = new El(tag || "div", id); byId[id] = e; return e; }

const body = new El("body", "");
for (const id of ["selection", "toast", "popover", "tag-text", "quote-block", "pen-chip"]) make(id);
const severity = make("severity-chips");
severity.dataset.value = "medium";
for (const v of ["high", "medium", "low"]) {
  const c = new El("button");
  c.className = v === "medium" ? "chip selected" : "chip";
  c.dataset.value = v;
  c.textContent = v;
  severity.appendChild(c);
}
const area = make("area-chips");
area.dataset.value = "other";
for (const v of ["layout", "copy", "a11y", "behaviour", "other"]) {
  const c = new El("button");
  c.className = v === "other" ? "chip selected" : "chip";
  c.dataset.value = v;
  c.textContent = v;
  area.appendChild(c);
}
const targets = make("target-chips");
targets.dataset.value = "other";

const calls = [];
const listeners = {};
const sandbox = {
  console,
  setTimeout,
  clearTimeout,
  setInterval: () => 0,
  document: {
    body,
    getElementById: (id) => byId[id] || null,
    createElement: (tag) => new El(tag),
  },
  window: {
    innerWidth: 2496,
    innerHeight: 1664,
    addEventListener: (n, fn) => ((listeners["window:" + n] = listeners["window:" + n] || []).push(fn)),
    __TAURI__: {
      core: {
        invoke: (name, args) => {
          calls.push([name, args]);
          if (name === "get_armed") return Promise.resolve(true);
          if (name === "get_last_note") {
            return Promise.resolve({ text: "same as before", severity: "high", area: "copy", target: "helmsly" });
          }
          if (name === "save_tag") return Promise.resolve({ tagNumber: 7, sweepName: "s" });
          return Promise.resolve(null);
        },
      },
      event: {
        listen: (name, fn) => ((listeners[name] = listeners[name] || []).push(fn)),
      },
    },
  },
};
sandbox.window.window = sandbox.window;
sandbox.globalThis = sandbox;
vm.createContext(sandbox);
vm.runInContext(fs.readFileSync(path.join(REPO, "ui/main.js"), "utf8"), sandbox, { filename: "main.js" });

function emit(name, payload) { for (const fn of listeners[name] || []) fn({ payload }); }
function keydown(el, event) { el.fire("keydown", Object.assign({ preventDefault() {} }, event)); }

let failures = 0;
function check(label, cond, extra) {
  if (cond) { console.log("ok   " + label); } else { failures++; console.log("FAIL " + label + (extra ? "  " + JSON.stringify(extra) : "")); }
}
function lastCall(name) {
  for (let i = calls.length - 1; i >= 0; i--) if (calls[i][0] === name) return calls[i][1];
  return null;
}

// 1. The overlay boots and asks for the armed state.
check("boots and reports ui_loaded", calls.some((c) => c[0] === "ui_loaded"));

// 2. Armed: quiet, but ready.
emit("armed-changed", true);
check("armed class set", body.classList.contains("armed"));

// 3. A highlight offers the pen chip below right of the point, and tells
//    the backend the rectangle it drew.
emit("highlight-hint", { x: 100, y: 200 });
check("chip shown", body.classList.contains("chip-visible"));
check("chip placed below right", byId["pen-chip"].style.left === "114px" && byId["pen-chip"].style.top === "214px",
  [byId["pen-chip"].style.left, byId["pen-chip"].style.top]);
check("chip rect sent", JSON.stringify(lastCall("set_chip_rect")) === JSON.stringify({ x: 114, y: 214, w: 28, h: 28 }), lastCall("set_chip_rect"));

// 4. Near the screen edge the chip stays on screen.
emit("highlight-hint", { x: 2490, y: 1660 });
check("chip clamped to the screen", byId["pen-chip"].style.left === "2464px" && byId["pen-chip"].style.top === "1632px",
  [byId["pen-chip"].style.left, byId["pen-chip"].style.top]);

// 5. A click elsewhere takes it away and the backend is told.
emit("chip-dismiss", null);
check("chip hidden on dismiss", !body.classList.contains("chip-visible"));
check("chip rect cleared", calls.some((c) => c[0] === "clear_chip_rect"));

// 6. A quote tag opens the popover with the quote block and target chips.
emit("entry-open", {
  x: 300, y: 400, w: 0, h: 0, kind: "quote",
  quote: "the save button sits below the fold\non a phone the form needs a scroll",
  tagNumber: 7, sweepName: "2026-09-07-default",
  targets: ["helmsly", "slobal.com", "AgnCred"], target: "helmsly",
});
check("entry open", body.classList.contains("entry"));
check("quote mode", body.classList.contains("quote-entry"));
check("quote text shown", byId["quote-block"].textContent.indexOf("the save button sits below the fold") === 0);
check("selection not drawn for a quote", byId["selection"].style.display === "none");
const targetValues = targets.querySelectorAll(".chip").map((c) => c.dataset.value);
check("target chips from settings with other last",
  JSON.stringify(targetValues) === JSON.stringify(["helmsly", "slobal.com", "AgnCred", "other"]), targetValues);
check("target preselected from the URL host", targets.dataset.value === "helmsly");

// 7. Ctrl+Up recalls the previous note and its chips.
keydown(byId["tag-text"], { key: "ArrowUp", ctrlKey: true });
setTimeout(() => {
  check("note recalled", byId["tag-text"].value === "same as before", byId["tag-text"].value);
  check("severity recalled", severity.dataset.value === "high");
  check("area recalled", area.dataset.value === "copy");
  check("target recalled", targets.dataset.value === "helmsly");

  // 8. Enter saves, carrying the target.
  byId["tag-text"].value = "wrong, it is pinned to the footer";
  keydown(byId["tag-text"], { key: "Enter", shiftKey: false });
  setTimeout(() => {
    const saved = lastCall("save_tag");
    check("save carries text, chips and target",
      saved && saved.text === "wrong, it is pinned to the footer" && saved.severity === "high" &&
      saved.area === "copy" && saved.target === "helmsly", saved);

    // 9. A region tag draws its rectangle and has no quote block.
    emit("entry-open", {
      x: 10, y: 20, w: 300, h: 200, kind: "region", quote: "",
      tagNumber: 8, sweepName: "s", targets: ["helmsly"], target: "",
    });
    check("region draws its rectangle", byId["selection"].style.display === "block");
    check("no quote block on a region tag", !body.classList.contains("quote-entry"));
    check("other selected when no target matched", targets.dataset.value === "other");

    // 10. The attach one shot says which tag it is going to.
    emit("one-shot", { on: true, message: "drag to attach to tag 7" });
    check("attach message shown", byId["toast"].textContent === "drag to attach to tag 7");
    emit("one-shot", true);
    check("plain one shot keeps its own words", byId["toast"].textContent === "Drag now to mark a region");

    // 11. A backend toast clears the capturing state.
    body.classList.add("capturing");
    emit("toast", { message: "attached to tag 7", duration: 1800 });
    check("toast clears capturing", !body.classList.contains("capturing"));
    check("toast text", byId["toast"].textContent === "attached to tag 7");

    console.log(failures === 0 ? "\nALL OVERLAY CHECKS PASSED" : "\n" + failures + " OVERLAY CHECKS FAILED");
    process.exit(failures === 0 ? 0 : 1);
  }, 10);
}, 10);
