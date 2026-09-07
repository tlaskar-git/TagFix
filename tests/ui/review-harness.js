// Minimal DOM shim so ui/review.js can be exercised in Node. Not a browser:
// only what review.js actually touches is implemented.
const fs = require("fs");
const path = require("path");
const vm = require("vm");

// The repo root, two levels up from tests/ui, so the harness runs from
// anywhere and moves with a clone.
const REPO = path.resolve(__dirname, "..", "..");

class ClassList {
  constructor() { this.set = new Set(); }
  add(c) { if (c) this.set.add(c); }
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
    this.checked = false;
    this.src = "";
    this._text = "";
  }
  set className(v) {
    this.classList = new ClassList();
    for (const c of String(v).split(/\s+/)) if (c) this.classList.add(c);
  }
  get className() { return this.classList.toString(); }
  set textContent(v) { this._text = String(v); this.children = []; }
  get textContent() { return this._text; }
  set innerHTML(v) { if (v === "") this.children = []; }
  get innerHTML() { return ""; }
  appendChild(child) { child.parent = this; this.children.push(child); return child; }
  insertBefore(child, ref) {
    this.children = this.children.filter((c) => c !== child);
    const at = ref ? this.children.indexOf(ref) : -1;
    if (at < 0) this.children.push(child); else this.children.splice(at, 0, child);
    child.parent = this;
    return child;
  }
  replaceChild(fresh, old) {
    const at = this.children.indexOf(old);
    if (at >= 0) this.children[at] = fresh; else this.children.push(fresh);
    fresh.parent = this;
    return old;
  }
  remove() { if (this.parent) this.parent.children = this.parent.children.filter((c) => c !== this); }
  addEventListener(name, fn) { (this.listeners[name] = this.listeners[name] || []).push(fn); }
  fire(name, event) { for (const fn of this.listeners[name] || []) fn(event || {}); }
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
  getBoundingClientRect() { return { top: 0, height: 10 }; }
  focus() { this.focused = true; }
  blur() { this.fire("blur"); }
  get text() { return this._text; }
}

function textOf(node) {
  let out = node._text || "";
  for (const c of node.children) out += " " + textOf(c);
  return out;
}

const byId = {};
function make(id, tag, cls) {
  const e = new El(tag || "div", id);
  if (cls) e.className = cls;
  byId[id] = e;
  return e;
}

// Mirrors ui/review.html.
make("sweep-select", "select");
make("tag-list", "ul");
make("status", "span");
make("new-sweep-btn", "button");
make("carry-btn", "button");
make("export-btn", "button");
make("copy-chat-btn", "button");
make("copy-text-btn", "button");
make("open-btn", "button");
make("open-menu", "div", "menu hidden");
make("save-btn", "button");
make("save-menu", "div", "menu hidden");
make("carry-panel", "section", "hidden");
make("carry-source", "select");
make("carry-list", "ul");
make("carry-do", "button");
make("carry-close", "button");
make("carry-status", "span");
make("lightbox", "div", "hidden");
make("lightbox-image", "img");

const regionTag = {
  number: 1, kind: "region", image: "tag-01.png", contextImage: "tag-01-context.png",
  quote: "", text: "button clipped", severity: "high", area: "layout", target: "helmsly",
  capturedUtc: "2026-09-07T10:00:00Z", windowTitle: "Helmsly", processName: "helmsly.exe",
  url: "slobal.com/portal", element: "button 'Deploy'", dropped: false,
  attachments: [{ image: "tag-01-a1.png", label: "compare" }], carriedFrom: null,
};
const quoteTag = {
  number: 2, kind: "quote", image: null, contextImage: null,
  quote: "the relay never moves", text: "wrong, DNS only", severity: "medium", area: "copy",
  target: "", capturedUtc: "2026-09-07T10:05:00Z", windowTitle: "Claude", processName: "claude.exe",
  url: "", element: "", dropped: false, attachments: [], carriedFrom: null,
};
const carriedTag = {
  number: 3, kind: "region", image: null, contextImage: null, quote: "",
  text: "still broken", severity: "low", area: "layout", target: "slobal.com",
  capturedUtc: "2026-09-07T10:10:00Z", windowTitle: "Edge", processName: "msedge.exe",
  url: "", element: "", dropped: false,
  attachments: [{ image: "tag-03-a1.png", label: "after" }],
  carriedFrom: { sweep: "2026-09-01-round-97", number: 4, image: "tag-03-before.png", text: "still broken" },
};

const calls = [];
const listeners = {};
const clipboard = [];

const sandbox = {
  console,
  setTimeout,
  clearTimeout,
  Promise,
  Set,
  navigator: { clipboard: { writeText: (t) => { clipboard.push(t); return Promise.resolve(); } } },
  document: {
    body: new El("body", ""),
    getElementById: (id) => byId[id] || null,
    createElement: (tag) => new El(tag),
  },
  window: {
    addEventListener: (n, fn) => ((listeners["window:" + n] = listeners["window:" + n] || []).push(fn)),
    __TAURI__: {
      core: {
        invoke: (name, args) => {
          calls.push([name, args]);
          if (name === "get_settings") {
            return Promise.resolve({ targets: [{ name: "helmsly" }, { name: "slobal.com" }, { name: "  " }] });
          }
          if (name === "list_sweeps") {
            return Promise.resolve([["2026-09-07-round-98", 3], ["2026-09-01-round-97", 2]]);
          }
          if (name === "load_sweep") {
            return Promise.resolve({ tags: [regionTag, quoteTag, carriedTag] });
          }
          if (name === "read_image") return Promise.resolve("QUJD");
          if (name === "render_export") return Promise.resolve("rendered " + args.fileName);
          if (name === "export_sweep") {
            return Promise.resolve({ pointer: "TagFix agent brief: x", targetDirs: ["D:/repos/helmsly/qa/2026-09-07-round-98"] });
          }
          if (name === "list_sweep_tags") {
            return Promise.resolve([
              { number: 1, text: "old one", kind: "region", image: "tag-01.png", dropped: false },
              { number: 2, text: "old two", kind: "quote", image: null, dropped: true },
              { number: 3, text: "old three", kind: "region", image: "tag-03.png", dropped: false },
            ]);
          }
          if (name === "carry_forward") return Promise.resolve([4, 5]);
          if (name === "open_export") return Promise.resolve("D:/sweeps/x/" + args.fileName);
          if (name === "save_export_as") return Promise.resolve("D:/chosen/" + args.fileName);
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
vm.runInContext(fs.readFileSync(path.join(REPO, "ui/review.js"), "utf8"), sandbox, {
  filename: "review.js",
});

let failures = 0;
function check(label, cond, extra) {
  if (cond) { console.log("ok   " + label); }
  else { failures++; console.log("FAIL " + label + (extra ? "  " + JSON.stringify(extra) : "")); }
}
function lastCall(name) {
  for (let i = calls.length - 1; i >= 0; i--) if (calls[i][0] === name) return calls[i][1];
  return null;
}
function callsNamed(name) {
  return calls.filter((c) => c[0] === name).map((c) => c[1]);
}
function tick(fn) { setTimeout(fn, 15); }

const rows = () => byId["tag-list"].children;

tick(() => {
  // 1. Boot: settings for the target list, the sweep list and the tags.
  check("asks for settings targets", calls.some((c) => c[0] === "get_settings"));
  check("lists sweeps", calls.some((c) => c[0] === "list_sweeps"));
  check("loads the newest sweep", lastCall("load_sweep") &&
    lastCall("load_sweep").dirName === "2026-09-07-round-98", lastCall("load_sweep"));

  // 2. A row per tag, with the right evidence for each kind.
  check("a row per tag", rows().length === 3, rows().length);

  const region = rows()[0];
  const quote = rows()[1];
  const carried = rows()[2];

  check("region row has a crop thumbnail",
    region.querySelectorAll(".thumb").length >= 1);
  check("region row shows the context frame",
    region.querySelectorAll(".thumb").some((t) => textOf(t).indexOf("context") >= 0));
  check("region row shows the comparison attachment with its label",
    region.querySelectorAll(".thumb").some((t) => textOf(t).indexOf("compare") >= 0));
  check("region meta carries url and element",
    textOf(region).indexOf("slobal.com/portal") >= 0 && textOf(region).indexOf("button 'Deploy'") >= 0);

  check("quote row renders the quote block",
    quote.querySelectorAll(".quote-thumb").length === 1);
  check("quote row has no crop thumbnail",
    quote.querySelectorAll(".thumb").length === 0);

  check("carried row names the source",
    textOf(carried).indexOf("Re-report of 2026-09-01-round-97 tag 04") >= 0);
  check("carried row shows before and after",
    carried.querySelectorAll(".thumb").some((t) => textOf(t).indexOf("before") >= 0) &&
    carried.querySelectorAll(".thumb").some((t) => textOf(t).indexOf("after") >= 0));
  const afterBtn = carried.querySelectorAll(".after-btn")[0];
  check("carried row offers Capture after", !!afterBtn);

  // 3. Capture after arms a one shot attachment and says so.
  afterBtn.fire("click");
  tick(() => {
    check("capture after asks for an after attachment",
      JSON.stringify(lastCall("attach_next_capture")) === JSON.stringify({ number: 3, label: "after" }),
      lastCall("attach_next_capture"));
    check("capture after status names the tag",
      byId["status"].textContent === "drag on screen to capture the after image for tag 3",
      byId["status"].textContent);

    // 4. Target select is populated from settings with other last, and an
    //    edit sends the target explicitly.
    const selects = region.querySelectorAll("select");
    check("three selects per row", selects.length === 3, selects.length);
    const targetSel = selects[2];
    check("target options come from settings with other last",
      JSON.stringify(targetSel.children.map((o) => o.value)) ===
        JSON.stringify(["helmsly", "slobal.com", "other"]),
      targetSel.children.map((o) => o.value));

    const sevSel = selects[0];
    sevSel.value = "low";
    sevSel.fire("change");
    tick(() => {
      const saved = lastCall("update_tag");
      check("a severity edit still sends the target",
        saved && saved.number === 1 && saved.severity === "low" && saved.target === "helmsly", saved);

      // An untargeted tag sends "other" rather than nothing.
      const quoteSel = quote.querySelectorAll("select")[1];
      quoteSel.value = "a11y";
      quoteSel.fire("change");
      tick(() => {
        const saved2 = lastCall("update_tag");
        check("an untargeted tag sends other explicitly",
          saved2 && saved2.number === 2 && saved2.area === "a11y" && saved2.target === "other", saved2);

        // 5. Copy for chat and Copy as text render on the fly.
        byId["copy-chat-btn"].fire("click");
        tick(() => {
          check("Copy for chat renders feedback.md",
            lastCall("render_export").fileName === "feedback.md", lastCall("render_export"));
          check("Copy for chat writes to the clipboard",
            clipboard[clipboard.length - 1] === "rendered feedback.md", clipboard);

          byId["copy-text-btn"].fire("click");
          tick(() => {
            check("Copy as text renders feedback.txt",
              lastCall("render_export").fileName === "feedback.txt", lastCall("render_export"));

            // 6. Open and Save as menus list the five files.
            const wanted = ["fixlist.md", "fixlist.html", "brief.md", "feedback.md", "feedback.txt"];
            const openItems = byId["open-menu"].children.map((i) => i.textContent);
            const saveItems = byId["save-menu"].children.map((i) => i.textContent);
            check("Open menu lists the five files",
              JSON.stringify(openItems) === JSON.stringify(wanted), openItems);
            check("Save as menu lists the five files",
              JSON.stringify(saveItems) === JSON.stringify(wanted), saveItems);
            byId["open-btn"].fire("click");
            check("Open menu shows on click", !byId["open-menu"].classList.contains("hidden"));
            byId["save-btn"].fire("click");
            check("opening one menu closes the other",
              byId["open-menu"].classList.contains("hidden") &&
              !byId["save-menu"].classList.contains("hidden"));

            byId["open-menu"].children[2].fire("click");
            tick(() => {
              check("Open picks the file it names",
                lastCall("open_export").fileName === "brief.md", lastCall("open_export"));
              byId["save-menu"].children[4].fire("click");
              tick(() => {
                check("Save as picks the file it names",
                  lastCall("save_export_as").fileName === "feedback.txt", lastCall("save_export_as"));

                // 7. Export names the target folders it wrote.
                byId["export-btn"].fire("click");
                tick(() => {
                  check("export status names the target folders",
                    byId["status"].textContent.indexOf("D:/repos/helmsly/qa/2026-09-07-round-98") >= 0,
                    byId["status"].textContent);

                  // 8. New sweep opens the Phase B prompt window.
                  byId["new-sweep-btn"].fire("click");
                  check("New sweep opens the prompt window",
                    calls.some((c) => c[0] === "new_sweep_prompt"));

                  // 9. Carry forward: earlier sweeps only, ticked numbers sent.
                  byId["carry-btn"].fire("click");
                  tick(() => {
                    check("carry panel opens", !byId["carry-panel"].classList.contains("hidden"));
                    const sources = byId["carry-source"].children.map((o) => o.value);
                    check("carry sources exclude the current sweep",
                      JSON.stringify(sources) === JSON.stringify(["2026-09-01-round-97"]), sources);
                    check("carry list shows live and dropped tags",
                      byId["carry-list"].children.length === 3, byId["carry-list"].children.length);
                    check("carry rows mark the dropped one",
                      byId["carry-list"].children[1].classList.contains("dropped-row"));

                    const ticks = byId["carry-list"].querySelectorAll(".carry-tick");
                    ticks[0].checked = true;
                    ticks[0].fire("change");
                    ticks[2].checked = true;
                    ticks[2].fire("change");
                    byId["carry-do"].fire("click");
                    tick(() => {
                      const sent = lastCall("carry_forward");
                      check("carry sends the ticked numbers to the current sweep",
                        sent && sent.source === "2026-09-01-round-97" &&
                        JSON.stringify(sent.numbers) === JSON.stringify([1, 3]) &&
                        sent.dest === "2026-09-07-round-98", sent);
                      check("carry reloads the rows",
                        callsNamed("load_sweep").length >= 2);

                      // 10. A sweeps-changed event refreshes the selector.
                      const before = callsNamed("list_sweeps").length;
                      for (const fn of listeners["sweeps-changed"] || []) {
                        fn({ payload: "2026-09-01-round-97" });
                      }
                      tick(() => {
                        check("sweeps-changed refreshes the selector",
                          callsNamed("list_sweeps").length > before);
                        check("sweeps-changed selects the named sweep",
                          byId["sweep-select"].value === "2026-09-01-round-97",
                          byId["sweep-select"].value);

                        console.log(failures === 0
                          ? "\nALL REVIEW CHECKS PASSED"
                          : "\n" + failures + " REVIEW CHECKS FAILED");
                        process.exit(failures === 0 ? 0 : 1);
                      });
                    });
                  });
                });
              });
            });
          });
        });
      });
    });
  });
});
