// Universal view for c2-prompt.html → window.C2Universal.mount(el, ctx).
//
// Same idea as the real UniversalView.svelte (open tasks across every workspace, grouped by
// priority, filter by context, click to open the task in its workspace), grown up:
//   - a stat strip (overdue / due this week / open / done)
//   - group by priority · project · context · due date · workspace
//   - workspace chips, context chips, "show done"
//   - complete a task without leaving the list (round mark, or `x`), with Undo
//   - keyboard first: j/k or arrows to move, Enter to open, x to complete
//   - header search filters it (and highlights hits); `is:done` reveals done tasks
// Rows are display-only reads of the lines (TodoTxt.parseTask); the text stays the source of truth.
//
// ctx = { docOf(ws), mutate(ws, fn), open(ws, line), toast(msg, undo), workspaces: [keys] }
(function () {
  const T = window.TodoTxt;
  const D = window.TxData;
  const UI = window.TxUI;

  const esc = (s) => s.replace(/[&<>"]/g, (c) => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;" })[c]);
  const GROUPS = [
    ["priority", "Priority"],
    ["due", "Due"],
    ["project", "Project"],
    ["context", "Context"],
    ["workspace", "Workspace"],
  ];
  const DUE_ORDER = ["Overdue", "Today", "This week", "Later", "No date"];

  function mount(el, ctx) {
    const keys = ctx.workspaces;
    const st = { group: "priority", showDone: false, ws: new Set(keys), ctx: null, q: T.parseQuery("") };

    el.innerHTML = `
      <div class="mx-auto max-w-5xl px-5 py-6">
        <h1 class="text-2xl font-bold tracking-tighter">Universal</h1>
        <p class="mt-1 font-sans text-sm text-base-content/70">Open tasks from every workspace. Click one to open it where it lives.</p>

        <div id="u-stats" class="mt-5 grid grid-cols-2 gap-3 md:grid-cols-4"></div>

        <div class="mt-5 flex flex-wrap items-center gap-x-4 gap-y-2">
          <div class="flex items-center gap-2 text-[11px] font-bold uppercase tracking-wider text-base-content/60">Group
            <div id="u-group" class="join">${GROUPS.map(([k, l]) => `<button type="button" class="btn btn-xs join-item" data-group="${k}">${l}</button>`).join("")}</div>
          </div>
          <div id="u-ws" class="flex flex-wrap gap-1.5"></div>
          <label class="ml-auto flex cursor-pointer items-center gap-2 text-xs"><input id="u-done" type="checkbox" class="toggle toggle-sm" /> show done</label>
        </div>
        <div id="u-ctx" class="mt-3 flex flex-wrap gap-1.5" role="group" aria-label="Filter by context"></div>

        <div id="u-list" class="mt-6 flex flex-col gap-7"></div>
        <div id="u-empty" class="hidden mt-10 rounded-box border-2 border-dashed border-base-content/30 p-8 text-center">
          <p class="font-bold">Nothing matches.</p>
          <p class="mt-1 font-sans text-sm text-base-content/70">Loosen the search or the filters above.</p>
          <button id="u-reset" type="button" class="btn btn-sm btn-outline mt-4 rounded-full">Reset filters</button>
        </div>
        <p class="mt-8 flex flex-wrap items-center gap-x-4 gap-y-1 text-[11px] text-base-content/60">
          <span><kbd class="kbd kbd-xs">j</kbd> <kbd class="kbd kbd-xs">k</kbd> move</span>
          <span><kbd class="kbd kbd-xs">↵</kbd> open</span>
          <span><kbd class="kbd kbd-xs">x</kbd> complete</span>
          <span><kbd class="kbd kbd-xs">/</kbd> search</span>
        </p>
      </div>`;
    const $ = (id) => el.querySelector("#" + id);

    // ---- data ----
    function collect() {
      const rows = [];
      for (const ws of keys) {
        ctx.docOf(ws).split("\n").forEach((text, i) => {
          if (!text.trim()) return;
          const t = T.parseTask(text);
          rows.push({ ws, n: i + 1, text, t, due: T.dueLabel(t.due, D.TODAY) });
        });
      }
      return rows;
    }

    const groupOf = {
      priority: (r) => (r.t.pri ? `(${r.t.pri})` : "No priority"),
      due: (r) => (!r.due ? "No date" : r.due.tone === "overdue" ? "Overdue" : r.due.days === 0 ? "Today" : r.due.days <= 7 ? "This week" : "Later"),
      project: (r) => r.t.projects[0] || "No project",
      context: (r) => r.t.contexts[0] || "No context",
      workspace: (r) => r.ws,
    };
    const groupRank = (kind, name) => {
      if (kind === "due") return DUE_ORDER.indexOf(name);
      if (kind === "workspace") return keys.indexOf(name);
      if (kind === "priority") return name.startsWith("(") ? name.charCodeAt(1) : 999;
      return name.startsWith("No ") ? 9999 : 0;
    };

    // ---- render ----
    function render() {
      const all = collect();
      const open = all.filter((r) => !r.t.done);
      const soon = open.filter((r) => r.due && r.due.days >= 0 && r.due.days <= 7).length;
      const overdue = open.filter((r) => r.due && r.due.days < 0).length;

      $("u-stats").innerHTML = [
        ["Overdue", overdue, overdue ? "text-error" : ""],
        ["Due this week", soon, soon ? "text-warning" : ""],
        ["t open", open.length, ""],
        ["x done", all.length - open.length, ""],
      ]
        // "t open" / "x done" keep the logo's two letters lowercase; the other labels are small caps.
        .map(([label, n, tone]) => `<div class="rounded-box border-2 border-base-content/20 px-4 py-3"><div class="text-3xl font-bold tracking-tighter tabular-nums ${tone}">${n}</div><div class="mt-0.5 text-[11px] font-bold tracking-wider text-base-content/60 ${/^[tx] /.test(label) ? "" : "uppercase"}">${label}</div></div>`)
        .join("");

      el.querySelectorAll("[data-group]").forEach((b) => {
        const on = b.dataset.group === st.group;
        b.classList.toggle("btn-primary", on);
        b.classList.toggle("btn-outline", !on);
      });
      $("u-ws").innerHTML = keys
        .map((k) => {
          const n = open.filter((r) => r.ws === k).length;
          const on = st.ws.has(k);
          return `<button type="button" class="btn btn-xs rounded-full ${on ? "btn-primary" : "btn-outline"}" data-ws="${k}" aria-pressed="${on}">${k} <span class="opacity-70">${n}</span></button>`;
        })
        .join("");
      $("u-done").checked = st.showDone;

      const inScope = all.filter((r) => st.ws.has(r.ws) && (!r.t.done || st.showDone || st.q.flags.done));
      const ctxCount = new Map();
      inScope.forEach((r) => r.t.contexts.forEach((c) => ctxCount.set(c, (ctxCount.get(c) || 0) + 1)));
      $("u-ctx").innerHTML = [...ctxCount.entries()]
        .sort((a, b) => b[1] - a[1] || a[0].localeCompare(b[0]))
        .map(([c, n]) => `<button type="button" class="btn btn-xs rounded-full font-mono ${st.ctx === c ? "btn-primary" : "btn-outline"}" data-ctx="${c}" aria-pressed="${st.ctx === c}">${c} <span class="opacity-70">${n}</span></button>`)
        .join("");

      const rows = inScope.filter((r) => (!st.ctx || r.t.contexts.includes(st.ctx)) && T.matchesQuery(r.text, st.q));
      const buckets = new Map();
      rows.forEach((r) => {
        const g = groupOf[st.group](r);
        if (!buckets.has(g)) buckets.set(g, []);
        buckets.get(g).push(r);
      });
      const order = [...buckets.keys()].sort((a, b) => groupRank(st.group, a) - groupRank(st.group, b) || a.localeCompare(b));
      const rank = (r) => (r.t.done ? 1e6 : 0) + (r.due ? r.due.days + 1000 : 5000) * 10 + (r.t.pri ? r.t.pri.charCodeAt(0) - 64 : 9);
      $("u-list").innerHTML = order
        .map((g) => {
          const list = buckets.get(g).sort((a, b) => rank(a) - rank(b) || a.n - b.n);
          return `<section aria-label="${esc(g)}">
            <h2 class="mb-2 flex items-center gap-3 font-bold">${groupLabel(g)}<span class="badge badge-sm badge-ghost">${list.length}</span><span class="h-0.5 flex-1 rounded-full bg-base-content/10"></span></h2>
            <div class="flex flex-col">${list.map(rowHTML).join("")}</div></section>`;
        })
        .join("");
      $("u-empty").classList.toggle("hidden", rows.length > 0);
      return rows.length;
    }

    function groupLabel(g) {
      if (st.group === "priority") return g.startsWith("(") ? `<span class="tok-priority text-lg">${g}</span>` : `<span class="text-base-content/60">${g}</span>`;
      if (st.group === "project") return g.startsWith("+") ? `<span class="tok-project">${esc(g)}</span>` : `<span class="text-base-content/60">${g}</span>`;
      if (st.group === "context") return g.startsWith("@") ? `<span class="tok-context">${esc(g)}</span>` : `<span class="text-base-content/60">${g}</span>`;
      if (st.group === "due") return `<span class="${g === "Overdue" ? "text-error" : g === "Today" ? "text-warning" : g === "No date" ? "text-base-content/60" : ""}">${g}</span>`;
      return `<span class="flex items-center gap-2">${tile(g)} ${esc(g)}</span>`;
    }

    const tile = (ws) => `<span class="grid size-6 shrink-0 place-items-center rounded-lg border-2 border-base-content/40 text-[10px] font-bold tracking-tighter" title="${esc(ws)}">${esc(ws.slice(0, 2))}</span>`;

    function rowHTML(r) {
      let segs = T.tokenize(r.t.body);
      if (st.q.active) segs = T.markRanges(segs, T.findRanges(r.t.body, st.q), "tx-hit");
      const body = segs.map((s) => (s.cls ? `<span class="${s.cls}">${esc(s.text)}</span>` : esc(s.text))).join("");
      const dueBadge = r.due && !r.t.done
        ? `<span class="badge badge-sm shrink-0 font-bold ${r.due.tone === "overdue" ? "badge-error" : r.due.tone === "today" ? "badge-warning" : r.due.tone === "soon" ? "badge-warning badge-soft" : "badge-ghost"}">${r.due.text}</span>`
        : "";
      const ref = r.t.ref && (D.WS_REFS[r.ws] || {})[r.t.ref];
      const refPill = ref && ref.kind === "progress" ? `<span class="tx-ref shrink-0" style="margin-left:0">${ref.done}/${ref.total}</span>` : ref ? `<span class="tx-ref shrink-0" style="margin-left:0">${UI.icon("note")}</span>` : "";
      const pri = st.group !== "priority" && r.t.pri ? `<span class="tok-priority shrink-0">(${r.t.pri})</span>` : "";
      return `<div class="u-row flex items-center gap-3 rounded-field px-2 py-1.5 ${r.t.done ? "opacity-55" : ""}" data-ws="${r.ws}" data-n="${r.n}">
        <button type="button" class="tx-mark-btn ${r.t.done ? "is-done" : ""}" data-act="toggle" tabindex="-1" aria-label="${r.t.done ? "Mark not done" : "Mark done"}">${UI.icon("x")}</button>
        <button type="button" class="u-open flex min-w-0 flex-1 items-center gap-3 text-left text-sm" data-act="open">${pri}<span class="min-w-0 flex-1 truncate ${r.t.done ? "line-through" : ""}">${body}</span></button>
        ${dueBadge}${refPill}${st.group === "workspace" ? "" : tile(r.ws)}
      </div>`;
    }

    // ---- actions ----
    function toggle(ws, n) {
      const flip = (doc) => {
        const L = doc.split("\n");
        L[n - 1] = T.toggleComplete(L[n - 1], D.TODAY);
        return L.join("\n");
      };
      const idx = [...el.querySelectorAll(".u-open")].indexOf(document.activeElement);
      const was = T.completedInfo(ctx.docOf(ws).split("\n")[n - 1]);
      ctx.mutate(ws, flip);
      ctx.toast(was ? `Reopened in ${ws}` : `Completed in ${ws}`, () => ctx.mutate(ws, flip));
      restoreFocus(idx);
    }
    function restoreFocus(idx) {
      if (idx < 0) return;
      const list = [...el.querySelectorAll(".u-open")];
      if (list.length) list[Math.min(idx, list.length - 1)].focus();
    }
    function move(from, dir) {
      const list = [...el.querySelectorAll(".u-open")];
      const at = list.indexOf(from);
      const next = list[Math.max(0, Math.min(list.length - 1, at + dir))];
      if (next) next.focus();
    }

    el.addEventListener("click", (e) => {
      const g = e.target.closest("[data-group]");
      const w = e.target.closest("[data-ws]:not(.u-row)");
      const c = e.target.closest("[data-ctx]");
      const act = e.target.closest("[data-act]");
      if (g) { st.group = g.dataset.group; ctx.refresh(); }
      else if (w) {
        st.ws.has(w.dataset.ws) ? (st.ws.size > 1 && st.ws.delete(w.dataset.ws)) : st.ws.add(w.dataset.ws);
        ctx.refresh();
      } else if (c) { st.ctx = st.ctx === c.dataset.ctx ? null : c.dataset.ctx; ctx.refresh(); }
      else if (act) {
        const row = act.closest(".u-row");
        const ws = row.dataset.ws, n = Number(row.dataset.n);
        act.dataset.act === "toggle" ? toggle(ws, n) : ctx.open(ws, n);
      }
    });
    $("u-done").addEventListener("change", (e) => { st.showDone = e.target.checked; ctx.refresh(); });
    $("u-reset").addEventListener("click", () => { st.ws = new Set(keys); st.ctx = null; ctx.clearSearch(); });
    $("u-list").addEventListener("keydown", (e) => {
      const btn = e.target.closest(".u-open");
      if (!btn) return;
      const row = btn.closest(".u-row");
      if (e.key === "ArrowDown" || e.key === "j") { e.preventDefault(); move(btn, 1); }
      else if (e.key === "ArrowUp" || e.key === "k") { e.preventDefault(); move(btn, -1); }
      else if (e.key === "Home") { e.preventDefault(); move(btn, -1e6); }
      else if (e.key === "End") { e.preventDefault(); move(btn, 1e6); }
      else if (e.key === "x") { e.preventDefault(); toggle(row.dataset.ws, Number(row.dataset.n)); }
    });

    ctx.refresh = () => api.render();
    const api = {
      render() {
        return (api.count = render());
      },
      /** Applies the header search; returns how many rows match. */
      setQuery(str) {
        st.q = T.parseQuery(str);
        return api.render();
      },
      focusFirst() {
        const first = el.querySelector(".u-open");
        if (first) first.focus();
        return !!first;
      },
      count: 0,
    };
    return api;
  }

  window.C2Universal = { mount };
})();
