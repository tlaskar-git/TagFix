// Renders the README screenshots from the shipped ui/ markup and CSS with
// sample content, through a headless Edge or Chrome that is already on the
// machine. The Tauri bridge is shimmed: invoke() answers from fixtures and
// listen() records handlers that the page fires after load, so app.js,
// review.js, settings.js and the overlay's main.js run unmodified.
//
//   node tools/render-screenshots.js
//
// Output lands in docs/. Scratch pages go to a temp folder, never into ui/,
// because tauri.conf.json bundles everything under ui/ into the exe. No
// network, no dependencies beyond Node and a browser.
//
// Why a browser and not the real windows: the build machine that produced
// these cannot show a WebView2 window, and a shimmed render of the real
// markup is honest about what ships where a mock-up would not be.
const fs = require("fs");
const os = require("os");
const path = require("path");
const { spawn, execSync } = require("child_process");

const REPO = path.resolve(__dirname, "..");
const UI = path.join(REPO, "ui");
const DOCS = path.join(REPO, "docs");
const OUT = path.join(os.tmpdir(), "tagfix-screenshots");

const BROWSER =
  process.env.TAGFIX_BROWSER ||
  [
    "C:/Program Files (x86)/Microsoft/Edge/Application/msedge.exe",
    "C:/Program Files/Microsoft/Edge/Application/msedge.exe",
    "C:/Program Files/Google/Chrome/Application/chrome.exe",
  ].find((p) => fs.existsSync(p));
if (!BROWSER) {
  console.error("no Edge or Chrome found; set TAGFIX_BROWSER to a browser exe");
  process.exit(2);
}

fs.rmSync(OUT, { recursive: true, force: true });
fs.mkdirSync(OUT, { recursive: true });
for (const f of fs.readdirSync(UI)) {
  fs.copyFileSync(path.join(UI, f), path.join(OUT, f));
}

// Headless Edge writes the PNG and then may never exit, so the file is
// polled for and the browser killed once its size is stable.
function shoot(file, w, h, out, budget) {
  const profile = path.join(OUT, "profile");
  fs.rmSync(profile, { recursive: true, force: true });
  if (fs.existsSync(out)) fs.unlinkSync(out);
  const child = spawn(
    BROWSER,
    [
      "--headless=new",
      "--disable-gpu",
      "--hide-scrollbars",
      "--no-first-run",
      "--user-data-dir=" + profile,
      "--window-size=" + w + "," + h,
      "--force-device-scale-factor=1",
      "--virtual-time-budget=" + (budget || 5000),
      "--screenshot=" + out,
      "file:///" + path.join(OUT, file).replace(/\\/g, "/"),
    ],
    { stdio: "ignore" }
  );
  const deadline = Date.now() + 60000;
  let last = -1;
  let stable = 0;
  while (Date.now() < deadline) {
    Atomics.wait(new Int32Array(new SharedArrayBuffer(4)), 0, 0, 500);
    if (fs.existsSync(out)) {
      const size = fs.statSync(out).size;
      if (size > 0 && size === last) {
        stable++;
        if (stable >= 2) break;
      } else {
        stable = 0;
      }
      last = size;
    }
  }
  try {
    if (process.platform === "win32") {
      execSync("taskkill /PID " + child.pid + " /T /F", { stdio: "ignore" });
    } else {
      child.kill();
    }
  } catch (e) {}
  if (!fs.existsSync(out)) throw new Error("no screenshot for " + file);
  console.log("wrote", path.relative(REPO, out), fs.statSync(out).size, "bytes");
}

// A sample app page: the backdrop of the region shot and the source of the
// sample crop and context frame.
const APP_CSS = `
  body{margin:0;background:#f4f4f6;font-family:"Segoe UI",sans-serif;color:#1c1c1f}
  .top{background:#fff;border-bottom:1px solid #e1e1e6;padding:12px 24px;font-weight:600;display:flex;gap:24px;align-items:center}
  .top span{color:#6b6b70;font-weight:400}
  .wrap{display:flex}
  .nav{width:180px;padding:18px 0}
  .nav div{padding:8px 24px;color:#4a4a50}
  .nav div.on{background:#e8e8ee;color:#1c1c1f;font-weight:600}
  .main{flex:1;padding:24px 32px;max-width:720px}
  h1{font-size:20px;margin:0 0 6px}
  p.lead{color:#6b6b70;margin:0 0 20px}
  .card{background:#fff;border:1px solid #e1e1e6;border-radius:8px;padding:18px 20px;margin-bottom:16px}
  label{display:block;font-size:13px;color:#4a4a50;margin-bottom:4px}
  input{width:100%;box-sizing:border-box;border:1px solid #cfcfd4;border-radius:4px;padding:7px 9px;font-size:13px;margin-bottom:14px}
  .row{display:flex;gap:10px;align-items:center}
  .btn{border:1px solid #cfcfd4;background:#fff;border-radius:4px;padding:7px 14px;font-size:13px}
  .btn.primary{background:#2f6fed;border-color:#2f6fed;color:#fff}
  .hint{font-size:12px;color:#8b8b90;margin-top:6px}
`;

const APP_BODY = `
  <div class="top">Sample app <span>Workspace settings</span></div>
  <div class="wrap">
    <div class="nav"><div>General</div><div class="on">Notifications</div><div>Members</div><div>Billing</div></div>
    <div class="main">
      <h1>Notifications</h1>
      <p class="lead">Choose where alerts are sent and how often.</p>
      <div class="card">
        <label>Alert email</label><input value="alerts@example.com">
        <label>Digest frequency</label><input value="Every morning at 08:00">
        <label>Escalation contact</label><input value="On call rota">
        <div class="row" id="save-row"><button class="btn primary">Save changes</button><button class="btn">Cancel</button></div>
        <div class="hint">Changes apply to every member of the workspace.</div>
      </div>
    </div>
  </div>
`;

function page(css, body) {
  return `<!DOCTYPE html><html><head><meta charset="utf-8"><style>${css}</style></head><body>${body}</body></html>`;
}

// The crop: just the save row of the card, at crop size.
fs.writeFileSync(
  path.join(OUT, "sample-crop.html"),
  page(
    APP_CSS + " body{background:#fff;padding:12px 20px}",
    `<div class="row"><button class="btn primary">Save changes</button><button class="btn">Cancel</button></div>
     <div class="hint">Changes apply to every member of the workspace.</div>`
  )
);

// The context frame: the whole sample page with the crop outlined in red.
fs.writeFileSync(
  path.join(OUT, "sample-context.html"),
  page(
    APP_CSS + " #mark{position:absolute;border:3px solid #e5484d;pointer-events:none}",
    APP_BODY +
      `<script>const r=document.getElementById("save-row").getBoundingClientRect();
       const m=document.createElement("div");m.id="mark";m.style.left=(r.left-12)+"px";m.style.top=(r.top-10)+"px";
       m.style.width=(r.width+24)+"px";m.style.height=(r.height+36)+"px";document.body.appendChild(m);</script>`
  )
);

// A sample chat page: the backdrop for the quote shots. One sentence is
// drawn as a selection.
const CHAT_CSS = `
  body{margin:0;background:#fff;font-family:"Segoe UI",sans-serif;color:#1c1c1f}
  .chat{max-width:760px;margin:0 auto;padding:36px 24px}
  .me{background:#f0f0f4;border-radius:12px;padding:12px 16px;margin:0 0 24px 120px;font-size:15px}
  .ai{font-size:15px;line-height:1.6}
  .ai h3{font-size:15px;margin:18px 0 6px}
  .sel{background:#b4d5fe}
`;
const CHAT_BODY = `
  <div class="chat">
    <div class="me">Review the notifications page on a phone and tell me what to fix.</div>
    <div class="ai">
      <p>Three things stand out at 390 px wide.</p>
      <h3>1. The form cannot be sent without scrolling</h3>
      <p><span class="sel" id="sel">The Save button sits below the fold on a phone; the form needs a scroll before it can be sent.</span> Pinning the save row to the bottom of the viewport would keep it in reach.</p>
      <h3>2. The digest field truncates</h3>
      <p>The value "Every morning at 08:00" is clipped to "Every morn" because the input has a fixed width from the desktop layout.</p>
      <h3>3. The navigation has no current page marker</h3>
      <p>On the desktop the selected item is shaded. On the phone the shading is dropped with the sidebar, so nothing says which page is open.</p>
    </div>
  </div>
`;

const TARGETS = ["helmsly", "slobal.com", "AgnCred"];

// Overlay shots: the real index.html over a backdrop, with the events the
// backend would have sent fired after load. `scene` picks what to show.
function overlayPage(scene) {
  const index = fs.readFileSync(path.join(UI, "index.html"), "utf8");
  const backdrop =
    scene === "region" ? { css: APP_CSS, body: APP_BODY } : { css: CHAT_CSS, body: CHAT_BODY };
  const shim = `
  <style>${backdrop.css}
    #backdrop{position:fixed;inset:0;z-index:-1;overflow:hidden}
    body{background:transparent !important}
  </style>
  <div id="backdrop">${backdrop.body}</div>
  <script>
    const handlers = {};
    window.__TAURI__ = {
      core: { invoke: async (name) => (name === "get_armed" ? true : null) },
      event: { listen: async (name, fn) => { handlers[name] = fn; return () => {}; } },
    };
    const fire = (name, payload) => handlers[name] && handlers[name]({ payload });
    setTimeout(() => {
      fire("armed-changed", true);
      const scene = ${JSON.stringify(scene)};
      const targets = ${JSON.stringify(TARGETS)};
      if (scene === "chip" || scene === "quote") {
        const r = document.getElementById("sel").getBoundingClientRect();
        const point = { x: Math.round(r.right - 40), y: Math.round(r.bottom) };
        if (scene === "chip") fire("highlight-hint", point);
        else fire("entry-open", {
          x: point.x - 120, y: point.y, w: 0, h: 0, kind: "quote",
          quote: document.getElementById("sel").textContent,
          tagNumber: 3, sweepName: "2026-09-07-review-notes", targets, target: "other",
        });
      } else {
        const r = document.getElementById("save-row").getBoundingClientRect();
        fire("entry-open", {
          x: Math.round(r.left - 12), y: Math.round(r.top - 10),
          w: Math.round(r.width + 24), h: Math.round(r.height + 36),
          kind: "region", quote: "", tagNumber: 2, sweepName: "2026-09-07-review-notes",
          targets, target: "helmsly",
        });
      }
      setTimeout(() => {
        const ta = document.getElementById("tag-text");
        if (scene === "quote") {
          ta.value = "Wrong: the button is pinned to the footer at every width, this was fixed in the last round.";
        }
        if (scene === "region") {
          ta.value = "Save row scrolls out of reach on a phone. Pin it to the bottom of the viewport.";
          document.querySelector('#severity-chips [data-value="high"]').click();
          document.querySelector('#area-chips [data-value="layout"]').click();
        }
        if (ta) ta.blur();
      }, 200);
    }, 300);
  </script>
  <script src="main.js"></script>`;
  return index.replace('<script src="main.js"></script>', shim);
}
fs.writeFileSync(path.join(OUT, "overlay-chip.html"), overlayPage("chip"));
fs.writeFileSync(path.join(OUT, "overlay-quote.html"), overlayPage("quote"));
fs.writeFileSync(path.join(OUT, "overlay-region.html"), overlayPage("region"));

// Sample pixels first: the window shots inline them as thumbnails.
shoot("sample-crop.html", 420, 110, path.join(OUT, "sample-crop.png"));
shoot("sample-context.html", 1100, 620, path.join(OUT, "sample-context.png"));
// The pen chip lives four seconds; a shorter virtual time budget keeps it
// on screen for the shot.
shoot("overlay-chip.html", 1180, 720, path.join(DOCS, "screenshot-chip.png"), 1500);
shoot("overlay-quote.html", 1180, 720, path.join(DOCS, "screenshot-quote.png"));
shoot("overlay-region.html", 1180, 820, path.join(DOCS, "screenshot-tagging.png"));

const crop64 = fs.readFileSync(path.join(OUT, "sample-crop.png")).toString("base64");
const ctx64 = fs.readFileSync(path.join(OUT, "sample-context.png")).toString("base64");

// One sweep with every kind of tag the review section can show: a region
// tag with crop, context frame and comparison attachment; a quote tag; a
// carried tag with before and after; and a dropped tag below the fold.
const TAGS = [
  {
    number: 1, kind: "region", image: "tag-01.png", contextImage: "tag-01-context.png",
    region: { x: 640, y: 388, width: 420, height: 110 }, quote: "",
    text: "Save row scrolls out of reach on a phone. Pin it to the bottom of the viewport.",
    severity: "high", area: "layout", target: "helmsly", capturedUtc: "2026-09-07T10:02:11Z",
    windowTitle: "Sample app", processName: "msedge.exe",
    url: "https://app.example.com/settings/notifications", element: "button 'Save changes'",
    screenResolution: "2496x1664", dpiScale: 1.5, monitorIndex: 0, dropped: false,
    attachments: [{ image: "tag-01-a1.png", label: "compare", region: { x: 0, y: 0, width: 420, height: 110 }, capturedUtc: "2026-09-07T10:03:40Z" }],
    carriedFrom: null,
  },
  {
    number: 2, kind: "quote", image: null, contextImage: null, region: null,
    quote: "The Save button sits below the fold on a phone; the form needs a scroll before it can be sent.",
    text: "Wrong: the button is pinned to the footer at every width, this was fixed in the last round.",
    severity: "medium", area: "copy", target: "other", capturedUtc: "2026-09-07T10:05:30Z",
    windowTitle: "Claude", processName: "claude.exe", url: "", element: "",
    screenResolution: "2496x1664", dpiScale: 1.5, monitorIndex: 0, dropped: false, attachments: [], carriedFrom: null,
  },
  {
    number: 3, kind: "region", image: null, contextImage: null,
    region: { x: 640, y: 388, width: 420, height: 110 }, quote: "",
    text: "Digest field clips its value at phone width.", severity: "medium", area: "layout", target: "helmsly",
    capturedUtc: "2026-09-07T10:06:02Z", windowTitle: "Sample app", processName: "msedge.exe",
    url: "https://app.example.com/settings/notifications", element: "edit 'Digest frequency'",
    screenResolution: "2496x1664", dpiScale: 1.5, monitorIndex: 0, dropped: false,
    attachments: [{ image: "tag-03-a1.png", label: "after", region: { x: 0, y: 0, width: 420, height: 110 }, capturedUtc: "2026-09-07T10:06:40Z" }],
    carriedFrom: { sweep: "2026-09-01-notifications", number: 4, image: "tag-03-before.png", text: "Digest field clips its value at phone width." },
  },
  {
    number: 4, kind: "region", image: "tag-04.png", contextImage: "tag-04-context.png",
    region: { x: 200, y: 120, width: 420, height: 110 }, quote: "",
    text: "Cancel looks identical to Save at a glance.", severity: "low", area: "layout", target: "helmsly",
    capturedUtc: "2026-09-07T10:08:15Z", windowTitle: "Sample app", processName: "msedge.exe",
    url: "https://app.example.com/settings/notifications", element: "button 'Cancel'",
    screenResolution: "2496x1664", dpiScale: 1.5, monitorIndex: 0, dropped: true, attachments: [], carriedFrom: null,
  },
];

const SETTINGS = {
  hotkey: "ctrl+shift+t", quoteHotkey: "ctrl+shift+q", attachHotkey: "ctrl+shift+a",
  outputDir: null, launchAtLogin: false, showPenChip: true, quoteScreenshot: false,
  contextFrame: true, newSweepEachDay: false, helpShown: true,
  targets: [
    { name: "helmsly", hosts: ["on.slobal.com", "localhost", "127.0.0.1"], exportDir: "D:\\repos\\helmsly\\qa\\tagfix" },
    { name: "slobal.com", hosts: ["slobal.com", "www.slobal.com"], exportDir: null },
    { name: "AgnCred", hosts: ["agncred.com", "www.agncred.com"], exportDir: null },
  ],
};

// The one window, app.html, at a given section. The shim answers every
// command the three section scripts issue at load.
function windowPage(section) {
  const html = fs.readFileSync(path.join(UI, "app.html"), "utf8");
  const shim = `
  <script>
    const TAGS = ${JSON.stringify(TAGS)};
    const CROP = ${JSON.stringify(crop64)};
    const CTX = ${JSON.stringify(ctx64)};
    window.__TAURI__ = {
      core: { invoke: async (name, args) => {
        if (name === "list_sweeps") return [["2026-09-07-review-notes", 4], ["2026-09-01-notifications", 6]];
        if (name === "load_sweep") return { schemaVersion: 2, slug: "review-notes", createdUtc: "2026-09-07T10:00:00Z", tags: TAGS };
        if (name === "get_settings") return ${JSON.stringify(SETTINGS)};
        if (name === "read_image") return /context/.test(args.image) ? CTX : CROP;
        return null;
      } },
      event: { listen: async () => () => {} },
    };
    setTimeout(() => { const s = document.getElementById("status"); if (s) s.textContent = "4 tags loaded"; }, 900);
  </script>
  <script src="review.js"></script>`;
  return html
    .replace('<script src="review.js"></script>', shim)
    .replace(
      '<script src="app.js"></script>',
      '<script src="app.js"></script><script>location.hash = ' + JSON.stringify("#" + section) + ";</script>"
    );
}
fs.writeFileSync(path.join(OUT, "window-review.html"), windowPage("review"));
fs.writeFileSync(path.join(OUT, "window-settings.html"), windowPage("settings"));
fs.writeFileSync(path.join(OUT, "window-help.html"), windowPage("help"));
shoot("window-review.html", 1040, 780, path.join(DOCS, "screenshot-review.png"));
shoot("window-settings.html", 1040, 780, path.join(DOCS, "screenshot-settings.png"));
shoot("window-help.html", 1040, 780, path.join(DOCS, "screenshot-help.png"));
console.log("done");
