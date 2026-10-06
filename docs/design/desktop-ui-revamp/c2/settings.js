// Settings page for c2-prompt.html → window.C2Settings.mount(el, ctx).
//
// Folds the real app's "Devices & agents" screen (pairing, capability tokens, activity feed) into
// one settings page next to what today has no home: theme, pin on top, the menu-bar behavior,
// workspaces, shortcuts. Rows tagged "proposed" do not exist in the app yet; everything else maps
// to a real feature (README "Capabilities", help page, WorkspaceSwitcher).
//   Ref (DaisyUI form controls): https://daisyui.com/components/toggle/ · /range/ · /select/ · /checkbox/
//
// Live: theme, text size, line height, font, line numbers and the 100-char hint apply to the real
// buffer and to a preview. Pairing is a small state machine. Tokens can be created (secret shown
// once) and revoked. The header search filters these cards.
//
// ctx = { editorHost, workspaces: {key: {name, path}}, docOf(ws), current(), switchWorkspace(k),
//         openTasks(), toast(msg, undo), getPin(), setPin(on), themePref(), setThemePref(p),
//         flashDaemon() }
(function () {
  const T = window.TodoTxt;
  const D = window.TxData;
  const UI = window.TxUI;

  const esc = (s) => String(s).replace(/[&<>"]/g, (c) => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;" })[c]);
  const proposed = `<span class="badge badge-xs badge-outline font-normal">proposed</span>`;

  const row = (label, desc, control, { isNew = false } = {}) => `
    <div class="flex flex-wrap items-center gap-x-6 gap-y-2 border-t-2 border-base-content/10 py-3 first:border-t-0 first:pt-0 last:pb-0">
      <div class="min-w-0 flex-1 basis-64">
        <div class="flex items-center gap-2 text-sm font-bold">${label}${isNew ? proposed : ""}</div>
        ${desc ? `<p class="mt-0.5 font-sans text-xs leading-relaxed text-base-content/65">${desc}</p>` : ""}
      </div>
      <div class="shrink-0">${control}</div>
    </div>`;

  const card = (id, title, kw, body, right = "") => `
    <section id="s-${id}" data-s data-kw="${esc(kw)}" class="scroll-mt-4 rounded-box border-2 border-base-content/20 p-5">
      <div class="mb-4 flex items-center gap-3"><h2 class="text-lg font-bold tracking-tight">${title}</h2>${right}</div>
      <div>${body}</div>
    </section>`;

  const seg = (name, opts) => `<div class="join" data-seg="${name}">${opts.map(([v, l]) => `<button type="button" class="btn btn-xs join-item" data-v="${v}">${l}</button>`).join("")}</div>`;
  const toggle = (id, on) => `<input id="${id}" type="checkbox" class="toggle" ${on ? "checked" : ""} />`;
  const kbd = (...ks) => ks.map((k) => `<kbd class="kbd kbd-sm">${k}</kbd>`).join(" ");

  const SECTIONS = [
    ["general", "General"],
    ["appearance", "Appearance"],
    ["workspaces", "Workspaces"],
    ["shortcuts", "Shortcuts"],
    ["devices", "Devices"],
    ["tokens", "Tokens"],
    ["activity", "Activity"],
  ];

  const SCOPES = [
    ["read", "See every task"],
    ["write:add", "Add tasks"],
    ["write:complete", "Complete tasks"],
    ["write:edit", "Edit task text"],
    ["write:delete", "Delete tasks"],
    ["raw", "Raw file access. Skips every restriction below."],
  ];

  // Placeholder pairing pattern with rounded modules (not scannable, and says so).
  function qr(seed = 11, n = 25, c = 8) {
    let s = seed;
    const rnd = () => ((s = (s * 1664525 + 1013904223) >>> 0) / 2 ** 32);
    const corner = (r, k) => (r < 8 && k < 8) || (r < 8 && k >= n - 8) || (r >= n - 8 && k < 8);
    let out = "";
    for (let r = 0; r < n; r++) for (let k = 0; k < n; k++) if (!corner(r, k) && rnd() > 0.5) out += `<rect x="${k * c + 0.5}" y="${r * c + 0.5}" width="${c - 1}" height="${c - 1}" rx="2.4"/>`;
    const finder = (x, y) => `<rect x="${(x + 0.5) * c}" y="${(y + 0.5) * c}" width="${6 * c}" height="${6 * c}" rx="${1.8 * c}" fill="none" stroke="currentColor" stroke-width="${c}"/><rect x="${(x + 2) * c}" y="${(y + 2) * c}" width="${3 * c}" height="${3 * c}" rx="${c}"/>`;
    return `<svg viewBox="0 0 ${n * c} ${n * c}" fill="currentColor" role="img" aria-label="Pairing code placeholder">${out}${finder(0, 0)}${finder(n - 7, 0)}${finder(0, n - 7)}</svg>`;
  }

  function mount(el, ctx) {
    const prefs = { font: "jb", size: 14, lh: 26, numbers: true, hint: true };
    const devices = [
      { name: "This Mac", kind: "laptop", seen: "now", me: true },
      { name: "mara-laptop", kind: "laptop", seen: "2m ago" },
      { name: "pixel-9", kind: "phone", seen: "yesterday" },
    ];
    const tokens = [
      { name: "claude-desk", scopes: ["read", "write:edit"], limits: ["file:todo.txt"], used: "4m ago" },
      { name: "phone-shortcut", scopes: ["write:add"], limits: [], used: "yesterday" },
      { name: "ci-agent", scopes: ["read"], limits: ["project:txtodo"], used: "3d ago" },
    ];
    const activity = [
      ["mara-laptop", "completed", "pay the electricity bill", "2m"],
      ["claude-desk", "edited", "line 3 · draft the release notes", "4m"],
      ["phone-shortcut", "added", "buy milk and oat bars", "1h"],
      ["This Mac", "edited", "plan the Q4 roadmap", "2h"],
      ["mara-laptop", "added", "book flights for the offsite", "5h"],
      ["ci-agent", "read", "todo.txt", "yesterday"],
      ["pixel-9", "completed", "water the plants", "yesterday"],
      ["This Mac", "deleted", "an old draft line", "2d"],
    ];

    el.innerHTML = `
      <div class="mx-auto flex max-w-6xl gap-8 px-5 py-6">
        <nav id="s-nav" class="sticky top-6 hidden h-fit w-44 shrink-0 flex-col gap-1 md:flex" aria-label="Settings sections">
          <h1 class="mb-3 text-2xl font-bold tracking-tighter">Settings</h1>
          ${SECTIONS.map(([id, l]) => `<button type="button" class="s-nav-btn rounded-full px-3 py-1.5 text-left text-xs font-bold" data-go="${id}">${l}</button>`).join("")}
        </nav>
        <div class="min-w-0 flex-1 space-y-6">
          <h1 class="text-2xl font-bold tracking-tighter md:hidden">Settings</h1>
          ${general()}${appearance()}${workspacesCard()}${shortcuts()}${devicesCard()}${tokensCard()}${activityCard()}
          <p id="s-none" class="hidden rounded-box border-2 border-dashed border-base-content/30 p-8 text-center font-sans text-sm text-base-content/70">No settings match that search.</p>
        </div>
      </div>
      <dialog id="s-token-dlg" class="modal"><div class="modal-box max-w-lg rounded-box border-2 border-base-content bg-base-100 p-6"><div id="s-token-body"></div></div>
        <form method="dialog" class="modal-backdrop"><button aria-label="Close">close</button></form></dialog>`;
    const $ = (id) => el.querySelector("#" + id);
    const scroller = el; // the mount point is the scrolling view itself

    // ---- sections ----
    function general() {
      return card(
        "general",
        "General",
        "pin on top always menu bar tray close quit daemon save blur focus",
        row("Pin on top", "Keeps the window above others. Remembered between launches.", toggle("s-pin", ctx.getPin())) +
          row("Keep running in the menu bar", "Closing the window hides it. The tray icon's Quit is the only way out.", toggle("s-tray", true)) +
          row("Save when the buffer loses focus", "Off means only ⌘S saves. Esc still discards.", toggle("s-blur", true), { isNew: true }) +
          row("Daemon", `<span class="font-mono">txtodod</span> · <span class="font-mono">~/notes/.txtodo/txtodod.sock</span>`, `<div class="flex items-center gap-3"><span class="flex items-center gap-2 text-xs font-bold"><span class="status status-success"></span> connected</span><button id="s-restart" type="button" class="btn btn-xs btn-outline rounded-full">Restart</button></div>`)
      );
    }

    function appearance() {
      return card(
        "appearance",
        "Appearance",
        "theme dark light system font size text line height numbers gutter hint 100 characters preview",
        row("Theme", "System follows the OS, like the app does today.", seg("theme", [["system", "System"], ["light", "Light"], ["dark", "Dark"]])) +
          row("Editor font", "Only the buffer. The wordmark is always JetBrains Mono.", `<select id="s-font" class="select select-sm rounded-full font-mono"><option value="jb">JetBrains Mono</option><option value="sys">System monospace</option></select>`, { isNew: true }) +
          row("Text size", "", `<div class="flex items-center gap-3"><input id="s-size" type="range" min="12" max="18" step="1" value="${prefs.size}" class="range range-xs w-40" /><span id="s-size-v" class="w-10 text-right text-xs tabular-nums">14px</span></div>`, { isNew: true }) +
          row("Line height", "", seg("lh", [["24", "Tight"], ["26", "Regular"], ["30", "Roomy"]]), { isNew: true }) +
          row("Line numbers", "", toggle("s-numbers", true), { isNew: true }) +
          row("Underline past 100 characters", "A gentle nudge to keep lines short. Nothing stops a longer line.", toggle("s-hint", true), { isNew: true }) +
          `<div class="mt-4"><div class="mb-2 text-[11px] font-bold uppercase tracking-wider text-base-content/60">Preview</div><div id="s-preview" class="h-36 rounded-box border-2 border-base-content/20"></div></div>`
      );
    }

    function workspacesCard() {
      return card(
        "workspaces",
        "Workspaces",
        "workspaces folders directories projects notes txtodo home add remove",
        `<div id="s-ws-list"></div>
         <div class="mt-4 flex flex-wrap items-center gap-3"><button id="s-ws-add" type="button" class="btn btn-sm btn-primary rounded-full">${UI.icon("plus", "size-4")} Add a folder</button>
         <span class="font-sans text-xs text-base-content/65">Workspaces you use from the CLI or TUI show up here on their own.</span></div>`
      );
    }

    function shortcuts() {
      const line = (keys, what, isNew) => `<tr><td class="w-52 whitespace-nowrap py-2">${keys}</td><td class="font-sans text-sm">${what}${isNew ? ` ${proposed}` : ""}</td></tr>`;
      const group = (t) => `<tr><td colspan="2" class="pb-1 pt-4 text-[11px] font-bold uppercase tracking-wider text-base-content/60 first:pt-0">${t}</td></tr>`;
      return card(
        "shortcuts",
        "Shortcuts",
        "keyboard shortcuts hotkeys keys quick add search commands",
        `<table class="w-full"><tbody>
          ${group("Anywhere")}${line(kbd("⌘", "⇧", "Space"), "Quick Add from any app")}
          ${group("Buffer")}${line(kbd("⌘", "↵"), "Open the detail view for the line")}${line(kbd("⌘", "S"), "Save now (otherwise it saves on blur)")}${line(kbd("Esc"), "Discard the edit")}${line(kbd("⌘", "Z"), "Undo")}
          ${group("Search")}${line(kbd("/"), "Focus search", true)}${line(kbd("↵") + " " + kbd("⇧", "↵"), "Next / previous match", true)}
          ${group("Go to")}${line(kbd("g") + " " + kbd("t") + " · " + kbd("g") + " " + kbd("u") + " · " + kbd("g") + " " + kbd("s"), "Tasks · Universal · Settings", true)}
          ${group("Universal")}${line(kbd("j") + " " + kbd("k"), "Move", true)}${line(kbd("x"), "Complete the focused task", true)}
        </tbody></table>
        <p class="mt-3 font-sans text-xs text-base-content/65">The global hotkey is fixed for now; changing it is planned.</p>`
      );
    }

    function devicesCard() {
      return card(
        "devices",
        "Devices",
        "devices pair pairing qr code scan sas words confirm laptop phone revoke sync",
        `<div class="grid gap-6 lg:grid-cols-[19rem_1fr]">
           <div id="s-pair" class="rounded-box border-2 border-base-content p-4"></div>
           <div id="s-device-list"></div>
         </div>`
      );
    }

    function tokensCard() {
      return card(
        "tokens",
        "Capability tokens",
        "tokens capability agent api read write scope restrict secret bearer revoke ci claude",
        `<p class="mb-3 font-sans text-sm text-base-content/70">Named, scoped, revocable. Hand an agent write access to one file and nothing else.</p>
         <div class="overflow-x-auto"><table class="table"><thead><tr class="font-mono"><th>name</th><th>scopes</th><th>limited to</th><th>last used</th><th></th></tr></thead><tbody id="s-token-rows" class="text-sm"></tbody></table></div>`,
        `<button id="s-token-new" type="button" class="btn btn-sm btn-outline ml-auto rounded-full">${UI.icon("key", "size-4")} New token</button>`
      );
    }

    function activityCard() {
      return card(
        "activity",
        "Activity",
        "activity feed history ops log who changed what recent",
        `<ul id="s-activity-list" class="font-mono text-sm"></ul>
         <p class="mt-3 font-sans text-xs text-base-content/65">The newest 200 ops across the workspace. One fetch, refreshed on demand.</p>`,
        `<button id="s-act-refresh" type="button" class="btn btn-sm btn-ghost btn-square ml-auto" aria-label="Refresh">${UI.icon("sync", "size-4")}</button>`
      );
    }

    // ---- appearance: live ----
    const preview = UI.createEditor($("s-preview"), {
      value: ["(A) 2026-09-14 plan the Q4 roadmap +work @desk due:2026-09-26", "x 2026-09-18 2026-09-11 finish the onboarding doc +work", "2026-09-19 renew the passport +admin @online rec:1y", "(C) 2026-09-08 write down every idea for the side project so it stops rattling, rank them by effort +ideas", ""].join("\n"),
      gutter: "numbers",
      refs: {},
      readOnly: true,
      ariaLabel: "Preview",
    });
    preview.el.style.setProperty("--tx-pad-y", "0.5rem");
    // A preview is for looking at: never let the wheel scroll it instead of the page.
    preview.el.style.pointerEvents = "none";
    preview.input.tabIndex = -1;

    function applyPrefs() {
      for (const host of [ctx.editorHost, preview.el]) {
        host.style.setProperty("--tx-fs", prefs.size + "px");
        host.style.setProperty("--tx-lh", prefs.lh + "px");
        host.style.setProperty("--tx-font", prefs.font === "jb" ? '"JetBrains Mono", ui-monospace, SFMono-Regular, Menlo, monospace' : "ui-monospace, SFMono-Regular, Menlo, monospace");
        host.classList.toggle("hide-numbers", !prefs.numbers);
        host.classList.toggle("no-long-hint", !prefs.hint);
      }
      $("s-size-v").textContent = prefs.size + "px";
    }
    function paintSeg(name, value) {
      el.querySelectorAll(`[data-seg="${name}"] [data-v]`).forEach((b) => {
        const on = b.dataset.v === String(value);
        b.classList.toggle("btn-primary", on);
        b.classList.toggle("btn-outline", !on);
      });
    }
    paintSeg("theme", ctx.themePref());
    paintSeg("lh", prefs.lh);
    applyPrefs();

    el.addEventListener("click", (e) => {
      const b = e.target.closest("[data-seg] [data-v]");
      if (!b) return;
      const name = b.parentElement.dataset.seg;
      if (name === "theme") ctx.setThemePref(b.dataset.v);
      if (name === "lh") { prefs.lh = Number(b.dataset.v); applyPrefs(); }
      paintSeg(name, b.dataset.v);
    });
    $("s-font").addEventListener("change", (e) => { prefs.font = e.target.value; applyPrefs(); });
    $("s-size").addEventListener("input", (e) => { prefs.size = Number(e.target.value); applyPrefs(); });
    $("s-numbers").addEventListener("change", (e) => { prefs.numbers = e.target.checked; applyPrefs(); });
    $("s-hint").addEventListener("change", (e) => { prefs.hint = e.target.checked; applyPrefs(); });
    $("s-pin").addEventListener("change", (e) => ctx.setPin(e.target.checked, { fromSettings: true }));
    $("s-restart").addEventListener("click", () => { ctx.toast("Restarting txtodod…"); ctx.flashDaemon(); });

    // ---- workspaces ----
    function renderWorkspaces() {
      const keys = Object.keys(ctx.workspaces);
      $("s-ws-list").innerHTML = keys
        .map((k) => {
          const w = ctx.workspaces[k];
          const open = T.stats(ctx.docOf(k)).open;
          const here = ctx.current() === k;
          return `<div class="flex flex-wrap items-center gap-3 border-t-2 border-base-content/10 py-3 first:border-t-0 first:pt-0" data-ws="${k}">
            <span class="grid size-9 shrink-0 place-items-center rounded-xl border-2 ${here ? "border-base-content bg-base-content text-base-100" : "border-base-content/40"} text-xs font-bold tracking-tighter">${esc(k.slice(0, 2))}</span>
            <div class="min-w-0 flex-1"><div class="flex items-center gap-2 text-sm font-bold">${esc(k)}${here ? `<span class="badge badge-xs badge-primary">current</span>` : ""}</div><div class="truncate text-xs text-base-content/60">${esc(w.path)}</div></div>
            <span class="badge badge-sm badge-outline tabular-nums">${open} open</span>
            <div class="flex items-center gap-1" data-actions>
              <button type="button" class="btn btn-xs btn-outline rounded-full" data-act="open">Open</button>
              <button type="button" class="btn btn-xs btn-ghost rounded-full" data-act="remove">Remove</button>
            </div></div>`;
        })
        .join("");
    }
    $("s-ws-list").addEventListener("click", (e) => {
      const btn = e.target.closest("[data-act]");
      if (!btn) return;
      const rowEl = btn.closest("[data-ws]");
      const k = rowEl.dataset.ws;
      if (btn.dataset.act === "open") { ctx.switchWorkspace(k); ctx.openTasks(); }
      else if (btn.dataset.act === "remove") {
        rowEl.querySelector("[data-actions]").innerHTML = `<span class="mr-1 font-sans text-xs text-base-content/70">Files stay on disk.</span><button type="button" class="btn btn-xs btn-error rounded-full" data-act="really">Remove</button><button type="button" class="btn btn-xs btn-ghost rounded-full" data-act="cancel">Cancel</button>`;
      } else if (btn.dataset.act === "cancel") renderWorkspaces();
      else if (btn.dataset.act === "really") ctx.toast(`Removed ${k} from the list (files untouched). Demo only.`) || renderWorkspaces();
    });
    $("s-ws-add").addEventListener("click", () => ctx.toast("The native folder picker opens here."));

    // ---- devices: list + pairing state machine ----
    function renderDevices() {
      $("s-device-list").innerHTML = `<div class="mb-2 text-[11px] font-bold uppercase tracking-wider text-base-content/60">Paired devices</div>` + devices
        .map(
          (d, i) => `<div class="flex items-center gap-3 border-t-2 border-base-content/10 py-2.5 first:border-t-0">
            <span class="grid size-9 place-items-center rounded-xl border-2 border-base-content/40">${UI.icon(d.kind === "phone" ? "phone" : "laptop")}</span>
            <div class="min-w-0 flex-1"><div class="flex items-center gap-2 text-sm font-bold">${esc(d.name)}${d.me ? `<span class="badge badge-xs badge-outline">this device</span>` : ""}</div><div class="text-xs text-base-content/60">last seen ${d.seen}</div></div>
            ${d.me ? "" : `<button type="button" class="btn btn-xs btn-ghost rounded-full" data-revoke="${i}">Revoke</button>`}</div>`
        )
        .join("");
    }
    $("s-device-list").addEventListener("click", (e) => {
      const b = e.target.closest("[data-revoke]");
      if (!b) return;
      const [gone] = devices.splice(Number(b.dataset.revoke), 1);
      renderDevices();
      ctx.toast(`Revoked ${gone.name}`, () => { devices.splice(Number(b.dataset.revoke), 0, gone); renderDevices(); });
    });

    const pair = { state: "idle", left: 60, timer: null, seed: 11 };
    function setPair(state) {
      pair.state = state;
      clearInterval(pair.timer);
      if (state === "code") {
        pair.left = 60;
        pair.timer = setInterval(() => {
          pair.left--;
          if (pair.left <= 0) setPair("expired");
          else {
            const p = $("s-pair-left");
            if (p) { p.value = pair.left; $("s-pair-sec").textContent = pair.left; }
          }
        }, 1000);
      }
      if (state === "waiting") pair.timer = setTimeout(() => setPair("paired"), 1800);
      renderPair();
    }
    function renderPair() {
      const head = `<h3 class="font-mono text-base font-bold tracking-tight">Pair a device</h3>`;
      const body = {
        idle: `<p class="mt-2 font-sans text-sm text-base-content/70">Add a phone or another computer. Nothing moves until you both confirm.</p>
               <div class="mt-4 flex flex-wrap gap-2"><button type="button" class="btn btn-sm btn-primary rounded-full" data-pair="code">Show a code</button><button type="button" class="btn btn-sm btn-outline rounded-full" data-pair="scan">Scan a code</button></div>`,
        code: `<div class="mx-auto mt-3 w-48 text-base-content">${qr(pair.seed)}</div>
               <div class="mt-3 flex items-center gap-2 text-xs text-base-content/70"><progress id="s-pair-left" class="progress progress-primary h-1.5 flex-1" value="${pair.left}" max="60"></progress><span class="whitespace-nowrap">expires in <span id="s-pair-sec" class="tabular-nums">${pair.left}</span>s</span></div>
               <p class="mt-2 font-sans text-xs text-base-content/60">Placeholder pattern, not scannable. The code never carries a group key.</p>
               <div class="mt-3 flex flex-wrap gap-2"><button type="button" class="btn btn-xs btn-ghost rounded-full" data-pair="idle">Cancel</button><button type="button" class="btn btn-xs btn-outline rounded-full" data-pair="sas">Demo: the other device scanned</button></div>`,
        expired: `<p class="mt-3 font-sans text-sm text-base-content/70">The code expired. They last 60 seconds.</p><button type="button" class="btn btn-sm btn-primary mt-3 rounded-full" data-pair="fresh">New code</button>`,
        sas: `<p class="mt-2 text-sm font-bold">Say these six words out loud</p>
              <div class="mt-2 flex flex-wrap gap-1.5 font-mono">${["otter", "lamp", "cedar", "nine", "quiet", "river"].map((w) => `<span class="badge badge-outline badge-lg">${w}</span>`).join("")}</div>
              <p class="mt-2 font-sans text-xs text-base-content/60">They must match on both screens. Confirming is always a deliberate second tap, on both devices.</p>
              <div class="mt-3 flex flex-wrap gap-2"><button type="button" class="btn btn-sm btn-primary rounded-full" data-pair="waiting">They match</button><button type="button" class="btn btn-sm btn-ghost rounded-full" data-pair="differ">They differ</button></div>`,
        waiting: `<div class="mt-4 flex items-center gap-3 text-sm"><span class="loading loading-spinner"></span> Confirmed here. Waiting on the other device…</div>`,
        paired: `<div class="mt-3 flex items-center gap-3">${UI.icon("check", "size-6 text-success")}<div class="text-sm"><b>Paired with pixel-9-pro</b><div class="text-xs text-base-content/60">Both devices confirmed.</div></div></div><button type="button" class="btn btn-sm btn-outline mt-3 rounded-full" data-pair="done">Done</button>`,
      }[pair.state];
      $("s-pair").innerHTML = head + body;
    }
    $("s-pair").addEventListener("click", (e) => {
      const b = e.target.closest("[data-pair]");
      if (!b) return;
      const to = b.dataset.pair;
      if (to === "scan") return ctx.toast("The camera opens here to scan the other device's code.");
      if (to === "fresh") { pair.seed += 7; return setPair("code"); }
      if (to === "code") pair.seed += 3;
      if (to === "differ") { ctx.toast("Codes differ. Pairing cancelled, nothing was shared."); return setPair("idle"); }
      if (to === "done") { devices.push({ name: "pixel-9-pro", kind: "phone", seen: "now" }); renderDevices(); return setPair("idle"); }
      setPair(to);
    });

    // ---- tokens ----
    const badges = (list, cls = "") => list.map((s) => `<span class="badge badge-sm badge-outline ${cls}">${esc(s)}</span>`).join(" ");
    function renderTokens() {
      $("s-token-rows").innerHTML = tokens.length
        ? tokens
            .map(
              (t, i) => `<tr><td class="whitespace-nowrap font-bold">${esc(t.name)}</td><td><div class="flex flex-wrap gap-1">${badges(t.scopes)}</div></td><td><div class="flex flex-wrap gap-1">${t.limits.length ? badges(t.limits) : `<span class="text-base-content/50">everything</span>`}</div></td><td class="whitespace-nowrap text-base-content/70">${t.used}</td><td class="text-right"><button type="button" class="btn btn-xs btn-ghost rounded-full" data-revoke-token="${i}">Revoke</button></td></tr>`
            )
            .join("")
        : `<tr><td colspan="5" class="py-6 text-center font-sans text-base-content/60">No tokens yet.</td></tr>`;
    }
    $("s-token-rows").addEventListener("click", (e) => {
      const b = e.target.closest("[data-revoke-token]");
      if (!b) return;
      const i = Number(b.dataset.revokeToken);
      const [gone] = tokens.splice(i, 1);
      renderTokens();
      ctx.toast(`Revoked ${gone.name}`, () => { tokens.splice(i, 0, gone); renderTokens(); });
    });

    const dlg = $("s-token-dlg");
    function tokenForm(err = "") {
      $("s-token-body").innerHTML = `
        <h3 class="font-mono text-lg font-bold tracking-tight">New capability token</h3>
        <p class="mt-1 font-sans text-sm text-base-content/70">Give it the least it needs. You can revoke it any time.</p>
        <form id="s-token-form" class="mt-4 flex flex-col gap-4" novalidate>
          <label class="flex flex-col gap-1.5 text-xs font-bold">Name<input name="name" class="input w-full rounded-field font-mono" placeholder="release-bot" autocomplete="off" /></label>
          <fieldset class="flex flex-col gap-1.5"><legend class="mb-1 text-xs font-bold">Scopes</legend>
            ${SCOPES.map(([s, d]) => `<label class="flex cursor-pointer items-start gap-3 text-sm"><input type="checkbox" name="scope" value="${s}" class="checkbox checkbox-sm mt-0.5" /><span><span class="font-mono font-bold">${s}</span> <span class="font-sans text-xs text-base-content/65">${d}</span></span></label>`).join("")}
          </fieldset>
          <fieldset class="grid grid-cols-3 gap-2"><legend class="mb-1 text-xs font-bold">Limit to (optional)</legend>
            <input name="project" class="input input-sm w-full rounded-field font-mono" placeholder="project:" /><input name="context" class="input input-sm w-full rounded-field font-mono" placeholder="context:" /><input name="file" class="input input-sm w-full rounded-field font-mono" placeholder="file:" />
          </fieldset>
          <p id="s-token-err" class="min-h-5 text-sm text-error" role="alert">${esc(err)}</p>
          <div class="flex justify-end gap-2"><button type="button" class="btn btn-ghost rounded-full" data-close>Cancel</button><button type="submit" class="btn btn-primary rounded-full">Create token</button></div>
        </form>`;
    }
    function randomSecret() {
      const abc = "ABCDEFGHJKMNPQRSTVWXYZ23456789";
      const b = crypto.getRandomValues(new Uint8Array(32));
      return "txd_" + [...b].map((x) => abc[x % abc.length]).join("");
    }
    $("s-token-new").addEventListener("click", () => { tokenForm(); dlg.showModal(); });
    $("s-token-body").addEventListener("click", (e) => { if (e.target.closest("[data-close]")) dlg.close(); });
    $("s-token-body").addEventListener("submit", (e) => {
      e.preventDefault();
      const f = new FormData(e.target);
      const name = String(f.get("name") || "").trim();
      const scopes = f.getAll("scope");
      if (!name) return tokenForm("Give the token a name.") || $("s-token-form").elements.name.focus();
      if (!scopes.length) return tokenForm("Pick at least one scope.") || ($("s-token-form").elements.name.value = name);
      const limits = ["project", "context", "file"].map((k) => [k, String(f.get(k) || "").trim()]).filter(([, v]) => v).map(([k, v]) => `${k}:${v}`);
      tokens.unshift({ name, scopes, limits, used: "never" });
      renderTokens();
      const secret = randomSecret();
      $("s-token-body").innerHTML = `
        <h3 class="font-mono text-lg font-bold tracking-tight">Token created</h3>
        <p class="mt-1 font-sans text-sm text-base-content/70">Copy the secret now. It is shown once and never again.</p>
        <div class="mt-4 flex items-center gap-2 rounded-field border-2 border-base-content p-3"><code id="s-secret" class="min-w-0 flex-1 break-all text-sm">${secret}</code><button type="button" class="btn btn-sm btn-outline rounded-full" data-copy>${UI.icon("copy", "size-4")} Copy</button></div>
        <div class="mt-3 flex flex-wrap gap-1">${badges(scopes)}${limits.length ? badges(limits, "badge-primary") : ""}</div>
        <div class="mt-5 flex justify-end"><button type="button" class="btn btn-primary rounded-full" data-close>Done</button></div>`;
    });
    $("s-token-body").addEventListener("click", async (e) => {
      if (!e.target.closest("[data-copy]")) return;
      try { await navigator.clipboard.writeText($("s-secret").textContent); ctx.toast("Secret copied."); } catch (_) { ctx.toast("Select the secret and copy it by hand."); }
    });

    // ---- activity ----
    function renderActivity(fresh = false) {
      const list = fresh ? [["This Mac", "refreshed", "the feed", "just now"], ...activity] : activity;
      const glyph = { completed: ["x", "tok-completion-marker"], edited: ["~", "tok-priority"], added: ["+", "tok-project"], deleted: ["−", "text-error"], read: ["›", "text-base-content/60"], refreshed: ["↻", "text-base-content/60"] };
      $("s-activity-list").innerHTML = list
        .map(([who, verb, what, when]) => {
          const [g, cls] = glyph[verb] || ["·", ""];
          return `<li class="flex items-center gap-3 border-t-2 border-base-content/10 py-2.5 first:border-t-0"><span class="${cls} w-3 text-center font-bold">${g}</span><span class="min-w-0 flex-1 truncate"><b>${esc(who)}</b> ${verb} <span class="text-base-content/70">${esc(what)}</span></span><span class="shrink-0 text-xs text-base-content/60">${when}</span></li>`;
        })
        .join("");
    }
    $("s-act-refresh").addEventListener("click", (e) => {
      const svg = e.currentTarget.querySelector("svg");
      svg.animate([{ transform: "rotate(0)" }, { transform: "rotate(360deg)" }], { duration: 500 });
      renderActivity(true);
    });

    // ---- section nav (scroll spy) + search ----
    el.querySelectorAll("[data-go]").forEach((b) =>
      b.addEventListener("click", () => api.scrollTo(b.dataset.go))
    );
    const spy = new IntersectionObserver(
      (entries) => {
        const vis = entries.filter((x) => x.isIntersecting).sort((a, b) => a.boundingClientRect.top - b.boundingClientRect.top)[0];
        if (vis) markNav(vis.target.id.slice(2));
      },
      { root: scroller, rootMargin: "0px 0px -65% 0px", threshold: 0 }
    );
    el.querySelectorAll("[data-s]").forEach((s) => spy.observe(s));
    function markNav(id) {
      el.querySelectorAll(".s-nav-btn").forEach((b) => {
        const on = b.dataset.go === id;
        b.classList.toggle("bg-base-content", on);
        b.classList.toggle("text-base-100", on);
      });
    }
    markNav("general");
    // The last card can never reach the top of a short scroll area, so pin the highlight to it
    // once the view is scrolled to the bottom.
    scroller.addEventListener("scroll", () => {
      if (scroller.scrollTop + scroller.clientHeight >= scroller.scrollHeight - 4) {
        const shown = [...el.querySelectorAll("[data-s]:not(.hidden)")];
        if (shown.length) markNav(shown[shown.length - 1].id.slice(2));
      }
    });

    const api = {
      render() {
        renderWorkspaces();
      },
      scrollTo(id) {
        const t = el.querySelector("#s-" + id);
        if (t) { t.scrollIntoView({ behavior: "smooth", block: "start" }); markNav(id); }
      },
      /** Header search: hides cards whose title/keywords miss any term. Returns how many remain. */
      setQuery(str) {
        const q = T.parseQuery(str);
        let n = 0;
        el.querySelectorAll("[data-s]").forEach((s) => {
          const hay = (s.dataset.kw + " " + s.querySelector("h2").textContent).toLowerCase();
          const hit = q.terms.every((w) => hay.includes(w));
          s.classList.toggle("hidden", !hit);
          const nav = el.querySelector(`[data-go="${s.id.slice(2)}"]`);
          if (nav) nav.classList.toggle("hidden", !hit);
          if (hit) n++;
        });
        $("s-none").classList.toggle("hidden", n > 0);
        return n;
      },
      syncPin(on) { $("s-pin").checked = on; },
      syncTheme(pref) { paintSeg("theme", pref); },
      sections: SECTIONS.map(([id]) => id),
    };
    renderWorkspaces();
    renderDevices();
    renderPair();
    renderTokens();
    renderActivity();
    return api;
  }

  window.C2Settings = { mount };
})();
