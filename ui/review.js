// The Review section of the one window: the sweep, its tags and the ways
// out of it. Vanilla JS, no bundler, no framework. Wrapped in an IIFE and
// scoped to its own section, because settings.js and app.js share the page.
(() => {
  if (!document.getElementById("section-review")) return;

  const { invoke } = window.__TAURI__.core;
  const { listen } = window.__TAURI__.event;

  // The five renderings Copy, Open and Save as work with. Kept in the same
  // order as export.rs EXPORT_FILES so the menus read the same as the folder.
  const EXPORT_FILES = [
    "fixlist.md",
    "fixlist.html",
    "brief.md",
    "feedback.md",
    "feedback.txt",
  ];

  const sweepSelect = document.getElementById("sweep-select");
  const tagList = document.getElementById("tag-list");
  const statusEl = document.getElementById("status");
  const newSweepBtn = document.getElementById("new-sweep-btn");
  const newSweepName = document.getElementById("new-sweep-name");
  const carryBtn = document.getElementById("carry-btn");
  const exportBtn = document.getElementById("export-btn");
  const copyChatBtn = document.getElementById("copy-chat-btn");
  const copyTextBtn = document.getElementById("copy-text-btn");
  const openBtn = document.getElementById("open-btn");
  const openMenu = document.getElementById("open-menu");
  const saveBtn = document.getElementById("save-btn");
  const saveMenu = document.getElementById("save-menu");
  const carryPanel = document.getElementById("carry-panel");
  const carrySource = document.getElementById("carry-source");
  const carryList = document.getElementById("carry-list");
  const carryDo = document.getElementById("carry-do");
  const carryClose = document.getElementById("carry-close");
  const carryStatus = document.getElementById("carry-status");
  const lightbox = document.getElementById("lightbox");
  const lightboxImage = document.getElementById("lightbox-image");

  let currentSweep = null;
  let dragRow = null;
  let tags = [];
  let sweepNames = [];
  // Target chip values from settings, with "other" appended. The overlay
  // popover uses the same list, so a tag edited here keeps a value the
  // popover would have offered.
  let targetNames = ["other"];

  function setStatus(msg) {
    statusEl.textContent = msg;
  }

  function pad2(n) {
    return String(n).padStart(2, "0");
  }

  async function loadTargets() {
    try {
      const settings = await invoke("get_settings");
      const names = (settings.targets || [])
        .map((t) => (t.name || "").trim())
        .filter((n) => n !== "");
      targetNames = names.concat(["other"]);
    } catch (err) {
      targetNames = ["other"];
    }
  }

  // Images come through a command rather than the asset protocol, so nothing
  // in this window can read a path it was not handed.
  async function imageDataUrl(dirName, image) {
    const b64 = await invoke("read_image", { dirName, image });
    return "data:image/png;base64," + b64;
  }

  function openLightbox(src) {
    lightboxImage.src = src;
    lightbox.classList.remove("hidden");
  }

  function closeLightbox() {
    lightbox.classList.add("hidden");
    lightboxImage.src = "";
  }

  // A thumbnail that enlarges on click. The element is returned straight
  // away and filled in when the bytes arrive.
  function thumbnail(image, label, className) {
    const wrap = document.createElement("figure");
    wrap.className = className || "thumb";
    const img = document.createElement("img");
    img.alt = label;
    wrap.appendChild(img);
    if (label) {
      const cap = document.createElement("figcaption");
      cap.textContent = label;
      wrap.appendChild(cap);
    }
    const sweep = currentSweep;
    imageDataUrl(sweep, image)
      .then((src) => {
        img.src = src;
        img.addEventListener("click", () => openLightbox(src));
      })
      .catch(() => {
        wrap.className = wrap.className + " missing";
        img.remove();
        const note = document.createElement("span");
        note.className = "missing-note";
        note.textContent = image + " missing";
        wrap.appendChild(note);
      });
    return wrap;
  }

  function selectOf(values, current, onChange) {
    const sel = document.createElement("select");
    for (const v of values) {
      const opt = document.createElement("option");
      opt.value = v;
      opt.textContent = v;
      if (current === v) opt.selected = true;
      sel.appendChild(opt);
    }
    sel.addEventListener("change", () => onChange(sel.value));
    return sel;
  }

  async function saveTagEdit(tag, changes) {
    const text = "text" in changes ? changes.text : tag.text;
    const severity = "severity" in changes ? changes.severity : tag.severity;
    const area = "area" in changes ? changes.area : tag.area;
    // Phase B made the backend keep an existing target when none is sent.
    // This window knows the target, so it always sends one.
    const target = "target" in changes ? changes.target : tag.target || "other";
    try {
      await invoke("update_tag", {
        dirName: currentSweep,
        number: tag.number,
        text,
        severity,
        area,
        target,
      });
      tag.text = text;
      tag.severity = severity;
      tag.area = area;
      tag.target = target;
      setStatus("saved tag " + tag.number);
    } catch (err) {
      setStatus("save failed: " + err);
    }
  }

  function metaLine(tag) {
    const parts = [];
    if (tag.image) parts.push(tag.image);
    parts.push(tag.capturedUtc);
    parts.push(tag.windowTitle + " (" + tag.processName + ")");
    if (tag.url) parts.push(tag.url);
    if (tag.element) parts.push(tag.element);
    return parts.join("  |  ");
  }

  function evidenceColumn(tag) {
    const col = document.createElement("div");
    col.className = "evidence";

    if (tag.kind === "quote" && tag.quote) {
      const block = document.createElement("blockquote");
      block.className = "quote-thumb";
      block.textContent = tag.quote;
      col.appendChild(block);
    } else if (tag.image) {
      col.appendChild(thumbnail(tag.image, "", "thumb"));
    }

    if (tag.contextImage) {
      col.appendChild(thumbnail(tag.contextImage, "context", "thumb small"));
    }

    const carried = tag.carriedFrom;
    if (carried) {
      const note = document.createElement("div");
      note.className = "re-report";
      note.textContent =
        "Re-report of " + carried.sweep + " tag " + pad2(carried.number);
      col.appendChild(note);
      if (carried.image) {
        col.appendChild(thumbnail(carried.image, "before", "thumb small"));
      }
    }

    for (const a of tag.attachments || []) {
      col.appendChild(thumbnail(a.image, a.label, "thumb small"));
    }

    if (carried) {
      const hasAfter = (tag.attachments || []).some((a) => a.label === "after");
      const btn = document.createElement("button");
      btn.className = "after-btn";
      btn.textContent = hasAfter ? "Capture after again" : "Capture after";
      btn.addEventListener("click", async () => {
        try {
          await invoke("attach_next_capture", {
            number: tag.number,
            label: "after",
          });
          setStatus(
            "drag on screen to capture the after image for tag " + tag.number
          );
        } catch (err) {
          setStatus("capture after failed: " + err);
        }
      });
      col.appendChild(btn);
    }

    return col;
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
    num.textContent = "tag " + pad2(tag.number);
    li.appendChild(num);

    li.appendChild(evidenceColumn(tag));

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
        if (refreshPending) {
          refreshPending = false;
          await refresh();
        } else {
          renderTags();
        }
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
    meta.textContent = metaLine(tag);
    body.appendChild(meta);
    li.appendChild(body);

    const controls = document.createElement("div");
    controls.className = "row-controls";
    controls.appendChild(
      selectOf(["high", "medium", "low"], tag.severity, (v) =>
        saveTagEdit(tag, { severity: v })
      )
    );
    controls.appendChild(
      selectOf(["layout", "copy", "a11y", "behaviour", "other"], tag.area, (v) =>
        saveTagEdit(tag, { area: v })
      )
    );
    controls.appendChild(
      selectOf(targetNames, tag.target || "other", (v) =>
        saveTagEdit(tag, { target: v })
      )
    );

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

    li.addEventListener("dragstart", (ev) => {
      // Chromium needs data set for the drag loop to actually start.
      ev.dataTransfer.setData("text/plain", String(tag.number));
      ev.dataTransfer.effectAllowed = "move";
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
      if (refreshPending) {
        refreshPending = false;
        await refresh();
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

  // Saves, tray opens and new sweeps can all ask for a reload at once. Run
  // them one after another: two interleaved runs clear and fill the same
  // selector and list over each other.
  let listChain = Promise.resolve();

  function loadSweepList(preferred) {
    const run = listChain.then(() => loadSweepListNow(preferred));
    listChain = run.catch(() => {});
    return run;
  }

  async function loadSweepListNow(preferred) {
    const sweeps = await invoke("list_sweeps");
    sweepNames = sweeps.map((s) => s[0]);
    sweepSelect.innerHTML = "";
    for (const [name, count] of sweeps) {
      const opt = document.createElement("option");
      opt.value = name;
      opt.textContent = name + " (" + count + " tags)";
      sweepSelect.appendChild(opt);
    }
    if (sweeps.length === 0) {
      currentSweep = null;
      setStatus("no sweeps yet");
      return;
    }
    const wanted =
      preferred && sweepNames.indexOf(preferred) >= 0 ? preferred : null;
    currentSweep = wanted || (sweepNames.indexOf(currentSweep) >= 0 ? currentSweep : sweepNames[0]);
    sweepSelect.value = currentSweep;
    await loadTags();
  }

  // Toolbar menus. Two small dropdowns rather than one dialog: Open and Save
  // as differ only in where the rendering lands.
  function buildMenu(menuEl, onPick) {
    menuEl.innerHTML = "";
    for (const name of EXPORT_FILES) {
      const item = document.createElement("button");
      item.className = "menu-item";
      item.dataset.file = name;
      item.textContent = name;
      item.addEventListener("click", () => {
        menuEl.classList.add("hidden");
        onPick(name);
      });
      menuEl.appendChild(item);
    }
  }

  function toggleMenu(menuEl) {
    const showing = menuEl.classList.contains("hidden");
    openMenu.classList.add("hidden");
    saveMenu.classList.add("hidden");
    if (showing) menuEl.classList.remove("hidden");
  }

  async function copyRendering(fileName, label) {
    if (!currentSweep) return;
    try {
      const text = await invoke("render_export", {
        dirName: currentSweep,
        fileName,
      });
      await navigator.clipboard.writeText(text);
      setStatus(label + " copied to the clipboard (" + fileName + ")");
    } catch (err) {
      setStatus("copy failed: " + err);
    }
  }

  buildMenu(openMenu, async (fileName) => {
    if (!currentSweep) return;
    try {
      const path = await invoke("open_export", {
        dirName: currentSweep,
        fileName,
      });
      setStatus("opened " + path);
    } catch (err) {
      setStatus("open failed: " + err);
    }
  });

  buildMenu(saveMenu, async (fileName) => {
    if (!currentSweep) return;
    try {
      const path = await invoke("save_export_as", {
        dirName: currentSweep,
        fileName,
      });
      setStatus(path ? "saved to " + path : "save cancelled");
    } catch (err) {
      setStatus("save failed: " + err);
    }
  });

  openBtn.addEventListener("click", () => toggleMenu(openMenu));
  saveBtn.addEventListener("click", () => toggleMenu(saveMenu));

  copyChatBtn.addEventListener("click", () =>
    copyRendering("feedback.md", "Feedback sheet")
  );
  copyTextBtn.addEventListener("click", () =>
    copyRendering("feedback.txt", "Feedback sheet as text")
  );

  // New sweep is an inline control: a name beside the button, Enter or the
  // button creates it, and the selector moves to the new sweep.
  async function createSweep() {
    const name = newSweepName.value.trim();
    if (name === "") {
      setStatus("give the sweep a name");
      newSweepName.focus();
      return;
    }
    try {
      const dirName = await invoke("create_sweep", { name });
      newSweepName.value = "";
      // The backend also emits sweeps-changed; refreshing here means the
      // selector is right whether or not this window heard that event.
      await loadSweepList(dirName);
      setStatus("created " + dirName);
    } catch (err) {
      setStatus("new sweep failed: " + err);
    }
  }

  newSweepBtn.addEventListener("click", createSweep);
  newSweepName.addEventListener("keydown", (event) => {
    if (event.key === "Enter") {
      event.preventDefault();
      createSweep();
    }
  });

  exportBtn.addEventListener("click", async () => {
    if (!currentSweep) return;
    try {
      const result = await invoke("export_sweep", { dirName: currentSweep });
      let copied = false;
      try {
        await navigator.clipboard.writeText(result.pointer);
        copied = true;
      } catch (clipErr) {
        copied = false;
      }
      let msg = "exported " + EXPORT_FILES.join(", ");
      if (result.targetDirs && result.targetDirs.length > 0) {
        msg += "; target copies in " + result.targetDirs.join(", ");
      }
      if (copied) msg += "; brief pointer copied to clipboard";
      setStatus(msg);
    } catch (err) {
      setStatus("export failed: " + err);
    }
  });

  // Carry forward panel. An earlier sweep on the left, its tags ticked on the
  // right; live and dropped both, because a re-report often starts from
  // something that was dropped last round.
  let carryTicks = new Set();

  async function loadCarrySources() {
    carrySource.innerHTML = "";
    const others = sweepNames.filter((n) => n !== currentSweep);
    for (const name of others) {
      const opt = document.createElement("option");
      opt.value = name;
      opt.textContent = name;
      carrySource.appendChild(opt);
    }
    if (others.length === 0) {
      carryStatus.textContent = "no earlier sweep to carry from";
      carryList.innerHTML = "";
      return;
    }
    carrySource.value = others[0];
    await loadCarryTags();
  }

  async function loadCarryTags() {
    carryTicks = new Set();
    carryList.innerHTML = "";
    carryStatus.textContent = "";
    const source = carrySource.value;
    let rows = [];
    try {
      rows = await invoke("list_sweep_tags", { dirName: source });
    } catch (err) {
      carryStatus.textContent = "could not read " + source + ": " + err;
      return;
    }
    for (const row of rows) {
      const li = document.createElement("li");
      li.className = "carry-row" + (row.dropped ? " dropped-row" : "");
      li.dataset.number = row.number;

      const box = document.createElement("input");
      box.type = "checkbox";
      box.className = "carry-tick";
      box.addEventListener("change", () => {
        if (box.checked) carryTicks.add(row.number);
        else carryTicks.delete(row.number);
        carryStatus.textContent = carryTicks.size + " ticked";
      });
      li.appendChild(box);

      const label = document.createElement("span");
      label.className = "carry-label";
      label.textContent =
        "tag " +
        pad2(row.number) +
        " [" +
        row.kind +
        "] " +
        (row.text || "no text") +
        (row.dropped ? " (dropped)" : "");
      li.appendChild(label);

      if (row.image) {
        const img = document.createElement("img");
        img.className = "carry-thumb";
        img.alt = "tag " + pad2(row.number);
        invoke("read_image", { dirName: source, image: row.image })
          .then((b64) => {
            img.src = "data:image/png;base64," + b64;
          })
          .catch(() => img.remove());
        li.appendChild(img);
      }

      carryList.appendChild(li);
    }
  }

  carryBtn.addEventListener("click", async () => {
    const hidden = carryPanel.classList.contains("hidden");
    if (!hidden) {
      carryPanel.classList.add("hidden");
      return;
    }
    carryPanel.classList.remove("hidden");
    await loadCarrySources();
  });

  carryClose.addEventListener("click", () => carryPanel.classList.add("hidden"));
  carrySource.addEventListener("change", loadCarryTags);

  carryDo.addEventListener("click", async () => {
    if (!currentSweep) return;
    const numbers = [...carryTicks].sort((a, b) => a - b);
    if (numbers.length === 0) {
      carryStatus.textContent = "tick at least one tag";
      return;
    }
    try {
      const carried = await invoke("carry_forward", {
        source: carrySource.value,
        numbers,
        dest: currentSweep,
      });
      carryStatus.textContent =
        "carried " + carried.length + " tags in as " + carried.join(", ");
      await loadTags();
      setStatus("carried forward " + carried.length + " tags");
    } catch (err) {
      carryStatus.textContent = "carry failed: " + err;
    }
  });

  sweepSelect.addEventListener("change", async () => {
    currentSweep = sweepSelect.value;
    await loadTags();
  });

  lightbox.addEventListener("click", closeLightbox);

  window.addEventListener("keydown", (event) => {
    if (event.key === "Escape") {
      if (!lightbox.classList.contains("hidden")) {
        closeLightbox();
        return;
      }
      openMenu.classList.add("hidden");
      saveMenu.classList.add("hidden");
    }
  });

  // A sweep created here, by the day rollover or by a carry forward.
  listen("sweeps-changed", (event) => {
    loadSweepList(event.payload);
  });

  // The window hides rather than closes, so without these the list keeps
  // whatever it loaded when the window was first opened and every tag saved
  // after that is missing until TagFix restarts. The sweep list is reloaded
  // too, for the counts and for a sweep the day rollover just started; the
  // selected sweep stays selected.
  let refreshPending = false;

  async function refresh() {
    // Rebuilding the rows under an open text edit would throw it away, so
    // wait for the edit to finish; its blur handler calls back in.
    const active = document.activeElement;
    if (dragRow || (active && active.tagName === "TEXTAREA" && tagList.contains(active))) {
      refreshPending = true;
      return;
    }
    try {
      await loadSweepList();
    } catch (err) {
      setStatus("refresh failed: " + err);
    }
  }

  listen("tags-changed", () => refresh());
  listen("show-section", (event) => {
    if (event.payload === "review") refresh();
  });

  loadTargets()
    .then(() => loadSweepList())
    .catch((err) => setStatus("could not load sweeps: " + err));
})();
