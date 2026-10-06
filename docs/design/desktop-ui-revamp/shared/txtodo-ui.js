// Shared UI engine for the mockups → window.TxUI. Depends on todotxt.js (window.TodoTxt) and
// data.js (window.TxData). Markup it emits uses DaisyUI 5 + Tailwind 4 classes, so the Tailwind
// browser build picks them up as they are inserted.
// Ref (Tailwind browser build): https://tailwindcss.com/docs/installation/play-cdn
// Ref (DaisyUI components):     https://daisyui.com/components/
(function () {
  const T = window.TodoTxt;
  const D = window.TxData;

  // ---- icons + mark ---------------------------------------------------------------------------

  // Monoline icons on a 24 grid. Stroke/caps/joins come from .tx-icon (txtodo.css), matching
  // assets/brand/tx-mark.svg's round caps. Paths are original, deliberately simple.
  const ICONS = {
    search: '<circle cx="11" cy="11" r="6.5"/><path d="M16 16l4.5 4.5"/>',
    plus: '<path d="M12 5v14M5 12h14"/>',
    sun: '<circle cx="12" cy="12" r="4"/><path d="M12 3v2M12 19v2M3 12h2M19 12h2M5.6 5.6l1.4 1.4M17 17l1.4 1.4M5.6 18.4L7 17M17 7l1.4-1.4"/>',
    moon: '<path d="M20 14.5A8 8 0 1 1 9.5 4a6.5 6.5 0 0 0 10.5 10.5z"/>',
    pin: '<path d="M9 3h6l-1 6 3 3H7l3-3-1-6zM12 12v9"/>',
    devices: '<rect x="3" y="5" width="12" height="9" rx="2"/><path d="M7 18h4M9 14v4"/><rect x="17" y="8" width="4" height="9" rx="1.5"/>',
    stack: '<path d="M12 4l8 4-8 4-8-4 8-4z"/><path d="M4 12l8 4 8-4M4 16l8 4 8-4"/>',
    help: '<circle cx="12" cy="12" r="9"/><path d="M9.5 9.5a2.5 2.5 0 1 1 3.5 2.3c-.7.4-1 1-1 1.7M12 17h.01"/>',
    left: '<path d="M15 6l-6 6 6 6"/>',
    right: '<path d="M9 6l6 6-6 6"/>',
    down: '<path d="M6 9l6 6 6-6"/>',
    back: '<path d="M19 12H5M11 6l-6 6 6 6"/>',
    folder: '<path d="M3 7a2 2 0 0 1 2-2h4l2 2h8a2 2 0 0 1 2 2v8a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2V7z"/>',
    file: '<path d="M7 3h7l5 5v11a2 2 0 0 1-2 2H7a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2z"/><path d="M14 3v5h5"/>',
    note: '<path d="M5 4h14v12l-4 4H5V4z"/><path d="M9 9h6M9 13h3"/>',
    check: '<path d="M5 12l4.5 4.5L19 7"/>',
    hook: '<path d="M4 9h10l5 6"/>',
    x: '<path d="M6 6l12 12M18 6L6 18"/>',
    sync: '<path d="M20 11a8 8 0 0 0-14-4L4 9M4 4v5h5M4 13a8 8 0 0 0 14 4l2-2M20 20v-5h-5"/>',
    warn: '<path d="M12 4l9 16H3L12 4zM12 10v4M12 17.5h.01"/>',
    enter: '<path d="M20 6v6a2 2 0 0 1-2 2H5M9 10l-4 4 4 4"/>',
    dots: '<path d="M5 12h.01M12 12h.01M19 12h.01"/>',
    list: '<path d="M4 7h16M4 12h16M4 17h10"/>',
    bolt: '<path d="M13 3L5 13h6l-1 8 8-10h-6l1-8z"/>',
    key: '<circle cx="8" cy="15" r="4"/><path d="M11 12l9-9M16 7l3 3"/>',
    laptop: '<rect x="5" y="5" width="14" height="10" rx="2"/><path d="M3 19h18"/>',
    phone: '<rect x="8" y="3" width="8" height="18" rx="2.5"/><path d="M11 17.5h2"/>',
    sliders: '<path d="M4 7h9M19 7h1M4 17h1M11 17h9"/><circle cx="16" cy="7" r="2.5"/><circle cx="8" cy="17" r="2.5"/>',
    copy: '<rect x="8" y="8" width="12" height="12" rx="2.5"/><path d="M16 8V6.5A2.5 2.5 0 0 0 13.5 4H6.5A2.5 2.5 0 0 0 4 6.5v7A2.5 2.5 0 0 0 6.5 16H8"/>',
    trash: '<path d="M5 7h14M10 7V5h4v2M7 7l1 12h8l1-12"/>',
  };

  function icon(name, cls = "") {
    return `<svg class="tx-icon ${cls}" viewBox="0 0 24 24" width="1.25em" height="1.25em" aria-hidden="true">${ICONS[name] || ""}</svg>`;
  }

  // The logo, copied from assets/brand/tx-mark.svg (stroke 7, round caps). `live` adds a dim ghost
  // copy plus a drawn-on copy whose four strokes fill in with setMarkProgress().
  const MARK_PATHS = [
    "M20 6 L20 42 C 20 47, 23 49, 28 49", // t stem
    "M8 20 L28 20 L40 34", // t crossbar, hooking down like a check
    "M58 22 L80 44", // x
    "M80 22 L58 44",
  ];
  function mark({ live = false, stroke = 7 } = {}) {
    const attrs = `fill="none" stroke="currentColor" stroke-width="${stroke}" stroke-linecap="round" stroke-linejoin="round"`;
    const ghost = live ? `<g opacity="0.16">${MARK_PATHS.map((d) => `<path d="${d}" ${attrs}/>`).join("")}</g>` : "";
    const body = MARK_PATHS.map(
      (d) => `<path d="${d}" ${attrs} ${live ? 'pathLength="1" stroke-dasharray="1 2" stroke-dashoffset="0"' : ""}/>`
    ).join("");
    return `<svg viewBox="0 0 100 64" role="img" aria-label="txtodo">${ghost}<g class="tx-mark-body">${body}</g></svg>`;
  }

  /** Fills the four strokes in order: ratio 0 = all hidden, 1 = whole mark drawn. */
  function setMarkProgress(el, ratio) {
    const paths = el.querySelectorAll(".tx-mark-body path");
    // First call jumps straight to the current state; only later changes animate.
    const first = !el.dataset.drawn;
    if (first) {
      el.dataset.drawn = "1";
      el.classList.add("no-anim");
      requestAnimationFrame(() => requestAnimationFrame(() => el.classList.remove("no-anim")));
    }
    paths.forEach((p, i) => {
      const part = Math.min(1, Math.max(0, ratio * paths.length - i));
      // 1.05, not 1: an offset of exactly 1 can leave a zero-length dash that round caps paint as a dot.
      p.style.strokeDashoffset = part <= 0 ? "1.05" : String(1 - part);
    });
  }

  function fillMarks(root = document) {
    root.querySelectorAll("[data-tx-mark]").forEach((el) => {
      el.classList.add("tx-mark");
      if (el.dataset.txMark === "live") el.classList.add("tx-mark-live");
      el.innerHTML = mark({ live: el.dataset.txMark === "live" });
    });
    root.querySelectorAll("[data-tx-icon]").forEach((el) => {
      el.outerHTML = icon(el.dataset.txIcon, el.className);
    });
  }

  // ---- the overlay editor ---------------------------------------------------------------------

  const esc = (s) => s.replace(/[&<>]/g, (c) => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;" })[c]);

  /**
   * Turns `host` into a todo.txt editor. A real <textarea> (native caret/selection/IME/undo) sits
   * over a highlighted mirror. Preserves the real app's contract (FileView.svelte):
   *   click → caret, type; blur or Cmd/Ctrl+S commits; Esc discards to the last saved text;
   *   Cmd/Ctrl+Enter or double-click opens the detail view for that line; hover highlights a line;
   *   trailing blank line shows "Add a line…"; done lines are dimmed + struck; ref: shows n/m or a
   *   notes glyph; lines past 100 chars get a wavy underline.
   * Ref (textarea):     https://developer.mozilla.org/en-US/docs/Web/HTML/Element/textarea
   * Ref (execCommand):  https://developer.mozilla.org/en-US/docs/Web/API/Document/execCommand
   *   (deprecated but still the only way to edit a textarea without wiping its undo stack)
   */
  function createEditor(host, opts = {}) {
    const o = {
      value: "",
      gutter: "none", // "none" | "numbers" | "marks"
      refs: D.REFS,
      singleLine: false,
      commitOnBlur: true,
      readOnly: false,
      placeholder: "Add a line…",
      ariaLabel: "todo.txt",
      onChange() {},
      onStatus() {},
      onCaret() {},
      onCommit() {},
      onDetail() {},
      onSubmit() {},
      onCancel() {},
      onSearch() {},
      ...opts,
    };

    host.classList.add("tx-editor");
    if (o.singleLine) host.classList.add("tx-single");
    if (o.gutter !== "none") host.classList.add("has-gutter", `gutter-${o.gutter}`);
    host.innerHTML =
      `<div class="tx-mirror" aria-hidden="true"><div class="tx-mirror-inner"></div></div>` +
      `<textarea class="tx-input" spellcheck="false" autocapitalize="off" autocomplete="off" autocorrect="off" wrap="off" aria-label="${esc(o.ariaLabel)}"></textarea>` +
      (o.gutter !== "none" ? `<div class="tx-gutter"><div class="tx-gutter-inner"></div></div>` : "");
    const input = host.querySelector(".tx-input");
    const inner = host.querySelector(".tx-mirror-inner");
    const gutterInner = host.querySelector(".tx-gutter-inner");

    let baseline = o.value;
    let status = "clean"; // clean | dirty | saving | saved
    let hoverN = 0;
    // Search: terms are AND-ed; hits are the 1-based numbers of matching lines. Non-matching lines
    // dim instead of vanishing, so the file stays a file (raw text, line numbers, caret intact).
    let q = T.parseQuery("");
    let hits = [];
    let hitIdx = -1;
    let activeN = 0;
    let timers = [];
    input.value = o.value;

    const setStatus = (s) => {
      if (s === status) return;
      status = s;
      host.dataset.status = s;
      o.onStatus(s);
    };
    const clearTimers = () => {
      timers.forEach(clearTimeout);
      timers = [];
    };

    function lineHTML(text, i, isLast) {
      let segs = T.tokenize(text);
      const done = T.completedInfo(text);
      if (done) segs = T.markStrike(segs, done.descriptionStart);
      if (text.length > T.LINE_LENGTH_HINT) segs = T.markTail(segs, T.LINE_LENGTH_HINT, "tx-long");
      const n = i + 1;
      const isHit = q.active && text.trim() !== "" && T.matchesQuery(text, q);
      if (q.active && isHit) segs = T.markRanges(segs, T.findRanges(text, q), "tx-hit");
      let body = segs.map((s) => (s.cls ? `<span class="${s.cls}">${esc(s.text)}</span>` : esc(s.text))).join("");
      if (text === "" && (o.singleLine || isLast)) body = `<span class="tx-placeholder">${esc(o.placeholder)}</span>`;
      const slug = T.refSlug(text);
      const ref = slug && o.refs[slug];
      if (ref) {
        body +=
          ref.kind === "progress"
            ? `<span class="tx-ref" aria-label="${ref.done} of ${ref.total} done">${ref.done}/${ref.total}</span>`
            : `<span class="tx-ref" aria-label="has notes">${icon("note")}</span>`;
      }
      const cls = [
        "tx-line",
        done ? "is-done" : "",
        q.active && !isHit && text.trim() !== "" ? "is-dim" : "",
        n === hits[hitIdx] ? "is-cur" : "",
        n === hoverN ? "is-hover" : "",
        n === activeN ? "is-active" : "",
      ];
      return `<div class="${cls.filter(Boolean).join(" ")}" data-n="${n}">${body}</div>`;
    }

    function gutterHTML(text, i) {
      const n = i + 1;
      const act = n === activeN ? " is-active" : "";
      if (o.gutter === "numbers") return `<div class="tx-g${act}" data-n="${n}">${n}</div>`;
      if (text.trim() === "") return `<div class="tx-g${act}" data-n="${n}"></div>`;
      const done = !!T.completedInfo(text);
      return (
        `<div class="tx-g${act}" data-n="${n}"><button type="button" class="tx-mark-btn${done ? " is-done" : ""}" data-n="${n}" ` +
        `aria-label="${done ? "Mark not done" : "Mark done"}" tabindex="-1">${icon("x")}</button></div>`
      );
    }

    function searchInfo() {
      return { active: q.active, total: hits.length, index: hitIdx + 1, lines: input.value.split("\n").length };
    }

    function render() {
      const lines = input.value.split("\n");
      hits = [];
      if (q.active) lines.forEach((t, i) => t.trim() !== "" && T.matchesQuery(t, q) && hits.push(i + 1));
      hitIdx = hits.length ? Math.max(0, Math.min(hitIdx, hits.length - 1)) : -1;
      inner.innerHTML = lines.map((t, i) => lineHTML(t, i, i === lines.length - 1)).join("");
      if (gutterInner) gutterInner.innerHTML = lines.map((t, i) => gutterHTML(t, i)).join("");
      syncScroll();
      if (q.active) o.onSearch(searchInfo());
    }

    /** Scrolls line `n` into the middle third if it is off-screen. Never touches focus or caret. */
    function reveal(n) {
      const lh = parseFloat(getComputedStyle(host).getPropertyValue("--tx-lh")) || 28;
      const padY = parseFloat(getComputedStyle(input).paddingTop) || 0;
      const top = padY + (n - 1) * lh;
      const view = input.clientHeight;
      if (top < input.scrollTop + lh || top + lh > input.scrollTop + view - lh * 2) {
        input.scrollTop = Math.max(0, top - view / 3);
      }
    }

    function syncScroll() {
      inner.style.transform = `translate(${-input.scrollLeft}px, ${-input.scrollTop}px)`;
      if (gutterInner) gutterInner.style.transform = `translateY(${-input.scrollTop}px)`;
    }

    // Toggle a class on one existing line instead of re-rendering (hover fires on every mousemove).
    function mark1(n, cls, on) {
      const el = inner.children[n - 1];
      if (el) el.classList.toggle(cls, on);
      const g = gutterInner && gutterInner.children[n - 1];
      if (g && cls === "is-active") g.classList.toggle(cls, on);
    }
    function setHover(n) {
      if (n === hoverN) return;
      if (hoverN) mark1(hoverN, "is-hover", false);
      hoverN = n;
      if (n) mark1(n, "is-hover", true);
    }
    function setActive(n) {
      if (n === activeN) return;
      if (activeN) mark1(activeN, "is-active", false);
      activeN = n;
      if (n) mark1(n, "is-active", true);
    }

    function caret() {
      const p = input.selectionStart;
      const before = input.value.slice(0, p);
      return { line: before.split("\n").length, col: p - (before.lastIndexOf("\n") + 1) + 1 };
    }
    function reportCaret() {
      const c = caret();
      setActive(document.activeElement === input ? c.line : 0);
      o.onCaret(c);
    }
    function lineAt(clientY) {
      const r = input.getBoundingClientRect();
      const lh = parseFloat(getComputedStyle(host).getPropertyValue("--tx-lh")) || 28;
      const padY = parseFloat(getComputedStyle(input).paddingTop) || 0;
      const n = Math.floor((clientY - r.top - padY + input.scrollTop) / lh) + 1;
      return n >= 1 && n <= input.value.split("\n").length ? n : 0;
    }
    function lineRange(n) {
      const lines = input.value.split("\n");
      let from = 0;
      for (let i = 0; i < n - 1; i++) from += lines[i].length + 1;
      return { from, to: from + lines[n - 1].length, text: lines[n - 1] };
    }

    /** Edits through execCommand so Cmd+Z keeps working; falls back to setRangeText. */
    function replaceRange(from, to, text, caretAt) {
      input.focus();
      input.setSelectionRange(from, to);
      let ok = false;
      try {
        ok = document.execCommand("insertText", false, text);
      } catch (_) {
        ok = false;
      }
      if (!ok) {
        input.setRangeText(text, from, to, "end");
        input.dispatchEvent(new Event("input", { bubbles: true }));
      }
      const at = caretAt == null ? from + text.length : caretAt;
      input.setSelectionRange(at, at);
    }

    function commit() {
      if (input.value === baseline) return;
      const next = input.value;
      baseline = next;
      clearTimers();
      setStatus("saving"); // stands in for the daemon round trip (Apply → Change)
      timers.push(
        setTimeout(() => {
          setStatus("saved");
          o.onCommit(next);
          timers.push(setTimeout(() => setStatus(input.value === baseline ? "clean" : "dirty"), 1600));
        }, 320)
      );
    }
    function discard() {
      if (input.value === baseline) return;
      clearTimers();
      input.value = baseline;
      render();
      setStatus("clean");
      o.onChange(input.value);
      reportCaret();
    }

    input.addEventListener("input", () => {
      if (o.singleLine && input.value.includes("\n")) input.value = input.value.replace(/\n+/g, " ");
      clearTimers();
      render();
      setStatus(input.value === baseline ? "clean" : "dirty");
      o.onChange(input.value);
      reportCaret();
    });
    input.addEventListener("scroll", syncScroll);
    ["keyup", "mouseup", "focus", "select"].forEach((ev) => input.addEventListener(ev, reportCaret));
    document.addEventListener("selectionchange", () => {
      if (document.activeElement === input) reportCaret();
    });
    input.addEventListener("blur", () => {
      setActive(0);
      if (!o.singleLine || o.commitOnBlur) commit();
    });
    input.addEventListener("keydown", (e) => {
      const mod = e.metaKey || e.ctrlKey;
      if (mod && e.key.toLowerCase() === "s") {
        e.preventDefault();
        commit();
      } else if (e.key === "Escape") {
        e.preventDefault();
        if (o.singleLine) o.onCancel();
        else discard();
      } else if (mod && e.key === "Enter" && !o.singleLine) {
        e.preventDefault();
        const n = caret().line;
        o.onDetail(n, lineRange(n).text);
      } else if (e.key === "Enter" && o.singleLine) {
        e.preventDefault();
        o.onSubmit(input.value);
      }
    });
    input.addEventListener("dblclick", (e) => {
      if (o.singleLine) return;
      const n = lineAt(e.clientY);
      if (!n) return;
      e.preventDefault();
      o.onDetail(n, lineRange(n).text);
    });
    input.addEventListener("mousemove", (e) => setHover(o.singleLine ? 0 : lineAt(e.clientY)));
    input.addEventListener("mouseleave", () => setHover(0));
    if (gutterInner) {
      gutterInner.addEventListener("click", (e) => {
        const btn = e.target.closest(".tx-mark-btn");
        if (!btn || o.readOnly) return;
        const { from, to, text } = lineRange(Number(btn.dataset.n));
        replaceRange(from, to, T.toggleComplete(text, D.TODAY));
        commit();
      });
    }

    const api = {
      el: host,
      input,
      getValue: () => input.value,
      setValue(v, { resetBaseline = true } = {}) {
        clearTimers();
        input.value = v;
        if (resetBaseline) baseline = v;
        setStatus(input.value === baseline ? "clean" : "dirty");
        render();
        o.onChange(input.value); // programmatic edits (Quick Add, discard) must refresh counts too
      },
      focus: () => input.focus(),
      commit,
      discard,
      status: () => status,
      setReadOnly(on) {
        o.readOnly = on;
        input.readOnly = on;
        host.classList.toggle("is-readonly", on);
      },
      setRefs(refs) {
        o.refs = refs;
        render();
      },
      goToLine(n) {
        const { from, to } = lineRange(n);
        input.focus();
        input.setSelectionRange(from, to);
        const lh = parseFloat(getComputedStyle(host).getPropertyValue("--tx-lh")) || 28;
        input.scrollTop = Math.max(0, (n - 3) * lh);
        reportCaret();
      },
      /** Applies a quick-add chip to the line under the caret (editPopoverLogic.ts applyChip). */
      applyChip(chip) {
        const c = caret();
        const { from, to, text } = lineRange(c.line);
        const res = T.applyChip(text, Math.max(0, input.selectionStart - from), chip, D.TODAY);
        replaceRange(from, to, res.text, from + res.caret);
      },
      lineCount: () => input.value.split("\n").length,
      /** Highlights matches and dims the rest; jumps to the first hit. "" clears. */
      setSearch(str) {
        q = T.parseQuery(str);
        hitIdx = 0;
        render();
        if (!q.active) o.onSearch(searchInfo());
        else if (hits.length) reveal(hits[hitIdx]);
        return searchInfo();
      },
      /** Steps to the next (+1) or previous (-1) matching line, wrapping. Focus stays put. */
      searchNav(dir = 1) {
        if (!hits.length) return searchInfo();
        hitIdx = (hitIdx + dir + hits.length) % hits.length;
        render();
        reveal(hits[hitIdx]);
        return searchInfo();
      },
      revealLine: reveal,
    };
    if (o.readOnly) api.setReadOnly(true);
    render();
    return api;
  }

  // ---- quick add ------------------------------------------------------------------------------

  const CHIPS = [
    ["A", "(A)"], ["B", "(B)"], ["C", "(C)"], ["x", "x"], ["+", "+"], ["@", "@"], ["due:", "due:"], ["t:", "t:"], ["rec:", "rec:"],
  ];

  /**
   * The Quick Add popover (EditPopover.svelte with taskRef=null): one single-line editor, the nine
   * token chips, an inline lenient-validation hint, Enter adds / Esc cancels. The layout around it
   * is the caller's; this only fills `container`.
   */
  function mountQuickAdd(container, opts = {}) {
    const o = { onSubmit() {}, onCancel() {}, prompt: "mark", chips: true, footer: true, addLabel: "Add", ...opts };
    const prompt = o.prompt === "mark" ? `<span class="tx-mark w-9 shrink-0" data-tx-mark></span>` : o.prompt === "chevron" ? `<span class="font-bold text-lg opacity-60">›</span>` : "";
    container.innerHTML = `
      <div class="flex items-center gap-3">
        ${prompt}
        <div class="tx-qa-editor tx-field flex-1 min-w-0"></div>
        <button type="button" class="tx-qa-add btn btn-primary">${o.addLabel} ${icon("enter", "size-4")}</button>
      </div>
      <p class="tx-qa-hint mt-2 min-h-5 pl-1 text-sm text-warning" role="status"></p>
      ${o.chips ? `<div class="tx-qa-chips flex flex-wrap gap-1.5">${CHIPS.map(([c, l]) => `<button type="button" class="btn btn-xs btn-outline" data-chip="${c}">${l}</button>`).join("")}</div>` : ""}
      ${o.footer ? `<p class="mt-3 text-xs text-base-content/60">The daemon stamps the date and <code>id:</code>. Enter adds. Esc cancels. Nothing here blocks a save.</p>` : ""}`;
    fillMarks(container);
    const hint = container.querySelector(".tx-qa-hint");
    let t;
    const ed = createEditor(container.querySelector(".tx-qa-editor"), {
      singleLine: true,
      commitOnBlur: false,
      placeholder: "buy milk +home @errand",
      ariaLabel: "New task",
      refs: {},
      onChange(v) {
        clearTimeout(t);
        t = setTimeout(() => (hint.textContent = T.strictHint(v) || ""), 150);
      },
      onSubmit(v) {
        submit(v);
      },
      onCancel: () => o.onCancel(),
    });
    const submit = (v) => {
      if (!v.trim()) return o.onCancel();
      o.onSubmit(v);
      api.reset();
    };
    container.querySelector(".tx-qa-add").addEventListener("click", () => submit(ed.getValue()));
    container.querySelectorAll("[data-chip]").forEach((b) => {
      // Keep focus in the editor: a page that shows the chips only while focus is inside would
      // otherwise hide them on mousedown, before the click lands.
      b.addEventListener("mousedown", (e) => e.preventDefault());
      b.addEventListener("click", () => {
        ed.applyChip(b.dataset.chip);
        ed.focus();
      });
    });
    const api = {
      editor: ed,
      focus: () => ed.focus(),
      reset() {
        ed.setValue("");
        hint.textContent = "";
      },
    };
    return api;
  }

  // ---- detail view ----------------------------------------------------------------------------

  /**
   * DetailView.svelte's content: breadcrumb, the pinned parent line (single-line, edits in place),
   * then exactly one of {sub-list editor, notes} — whichever the `ref:` already has, notes
   * otherwise. Double-click / Cmd+Enter on a sub-list line drills one level deeper. `container`
   * only needs a height; the caller picks page / panel / drawer.
   */
  function mountDetail(container, { line, workspace = "notes", onExit = () => {}, dense = false } = {}) {
    const stack = [{ text: line }];
    const view = () => stack[stack.length - 1];

    function render() {
      const step = view();
      const slug = T.refSlug(step.text);
      const data = (slug && D.DETAILS[slug]) || {};
      const hasList = !!data.list;
      const crumbs = [{ label: workspace, to: 0 }, ...stack.map((s, i) => ({ label: crumbLabel(s.text), to: i + 1 }))];
      const path = `~/${workspace}/${slug || "…"}/${hasList ? "todo.txt" : "notes.md"}`;
      const st = hasList ? T.stats(data.list) : null;
      container.innerHTML = `
        <div class="flex h-full min-h-0 flex-col gap-${dense ? 3 : 5}">
          <div class="flex items-center gap-2">
            <button type="button" class="tx-d-back btn btn-ghost btn-sm btn-square" aria-label="Back">${icon("back")}</button>
            <div class="breadcrumbs min-w-0 flex-1 text-sm"><ul>${crumbs
              .map((c, i) => `<li>${i === crumbs.length - 1 ? `<span class="font-bold">${esc(c.label)}</span>` : `<a data-to="${c.to}" class="tx-d-crumb">${esc(c.label)}</a>`}</li>`)
              .join("")}</ul></div>
          </div>
          <section aria-label="Parent task">
            <div class="tx-d-parent tx-field tx-field-strong"></div>
          </section>
          ${
            hasList
              ? `<section class="flex min-h-0 flex-1 flex-col gap-2" aria-label="Sub-list">
                   <div class="flex items-center gap-3">
                     <h2 class="font-bold tracking-tight">${st.done} of ${st.total} done</h2>
                     <progress class="progress progress-primary h-2 w-40" value="${st.done}" max="${st.total}"></progress>
                   </div>
                   <div class="tx-d-list min-h-40 flex-1 rounded-box border-2 border-base-content/20"></div>
                 </section>`
              : `<section class="flex flex-col gap-2" aria-label="Notes">
                   <h2 class="font-bold tracking-tight">Notes</h2>
                   <textarea class="tx-notes textarea rounded-box" placeholder="Nothing here yet. Typing creates ${esc(path)}">${esc(data.notes || "")}</textarea>
                 </section>`
          }
          <footer class="mt-auto flex items-center gap-2 text-xs text-base-content/60">${icon("folder", "size-4")}<span class="font-mono">${esc(path)}</span></footer>
        </div>`;

      container.querySelector(".tx-d-back").addEventListener("click", back);
      container.querySelectorAll(".tx-d-crumb").forEach((a) =>
        a.addEventListener("click", () => {
          stack.length = Number(a.dataset.to) || 0;
          stack.length ? render() : onExit();
        })
      );
      const parent = createEditor(container.querySelector(".tx-d-parent"), {
        value: step.text,
        singleLine: true,
        refs: {},
        ariaLabel: "Parent task",
        onSubmit: () => parent.input.blur(),
        onChange: (v) => (step.text = v),
      });
      if (hasList) {
        createEditor(container.querySelector(".tx-d-list"), {
          value: data.list,
          refs: {},
          gutter: "marks",
          ariaLabel: "Sub-list",
          onDetail(n, text) {
            if (!text.trim()) return;
            stack.push({ text });
            render();
          },
        });
      }
    }
    function back() {
      stack.pop();
      stack.length ? render() : onExit();
    }
    function crumbLabel(text) {
      return text.replace(/^x \d{4}-\d{2}-\d{2}( \d{4}-\d{2}-\d{2})? /, "").replace(/^\([A-Z]\) /, "").replace(/^\d{4}-\d{2}-\d{2} /, "").replace(/[+@]\S+|\S+:\S+/g, "").trim().slice(0, 28) || "…";
    }
    render();
    return { back, render };
  }

  // ---- adding a task --------------------------------------------------------------------------

  /** What the daemon does on `add`: stamp the creation date unless the line already has one. */
  function stampLine(v) {
    const m = v.match(/^(\([A-Z]\) )?(.*)$/);
    const dated = /^\d{4}-\d{2}-\d{2} /.test(m[2]);
    return `${m[1] || ""}${dated ? "" : D.TODAY + " "}${m[2]}`;
  }

  /** Appends above the trailing blank "Add a line…" slot, like an Apply{kind:"add"} would. */
  function appendTask(editor, line) {
    editor.setValue(editor.getValue().replace(/\n?$/, "\n") + stampLine(line) + "\n");
    editor.input.scrollTop = editor.input.scrollHeight;
  }

  // ---- chrome: theme, demo toggles, hotkeys ---------------------------------------------------

  function setTheme(name) {
    document.documentElement.dataset.theme = name === "dark" ? "txtodo-dark" : "txtodo";
    document.dispatchEvent(new CustomEvent("tx-theme", { detail: name }));
  }
  const isDark = () => document.documentElement.dataset.theme === "txtodo-dark";

  /** `defaultTheme`: "light" | "dark" | "auto" (follow the OS, like the real app's resolvedTheme). */
  function initChrome({ defaultTheme = "auto" } = {}) {
    const auto = window.matchMedia("(prefers-color-scheme: dark)").matches ? "dark" : "light";
    setTheme(defaultTheme === "auto" ? auto : defaultTheme);
    fillMarks();
    document.addEventListener("click", (e) => {
      if (e.target.closest("[data-tx-theme-toggle]")) setTheme(isDark() ? "light" : "dark");
    });
  }

  /**
   * `[data-demo="name"]` buttons toggle `[data-demo-target="name"]` (banners, empty states) and
   * call handlers[name](on). Lets a reviewer see states the real daemon would produce.
   */
  function bindDemo(handlers = {}) {
    document.querySelectorAll("[data-demo]").forEach((btn) => {
      btn.addEventListener("click", () => {
        const name = btn.dataset.demo;
        const on = btn.getAttribute("aria-pressed") !== "true";
        btn.setAttribute("aria-pressed", String(on));
        document.querySelectorAll(`[data-demo-target="${name}"]`).forEach((t) => t.classList.toggle("hidden", !on));
        if (handlers[name]) handlers[name](on);
      });
    });
  }

  /** The app's global Quick Add hotkey: Cmd/Ctrl+Shift+Space (src-tauri/src/quick_add.rs). */
  function hotkeys({ quickAdd }) {
    document.addEventListener("keydown", (e) => {
      if ((e.metaKey || e.ctrlKey) && e.shiftKey && e.code === "Space") {
        e.preventDefault();
        quickAdd();
      }
    });
  }

  window.TxUI = { icon, mark, setMarkProgress, fillMarks, createEditor, mountQuickAdd, mountDetail, appendTask, stampLine, initChrome, bindDemo, hotkeys, setTheme, isDark };
})();
