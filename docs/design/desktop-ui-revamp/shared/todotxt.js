// Pure todo.txt display logic shared by every mockup. No DOM, so `selftest.mjs` can run it in Node.
//
// This is a *display approximation* that mirrors (does not import) the real app:
//   - apps/desktop/src/lib/todotxt/lineInfo.ts        completedLineInfo(), findRefTag()
//   - apps/desktop/src/lib/components/editPopoverLogic.ts   toggleComplete(), applyChip()
//   - apps/desktop/src/lang/todotxt.grammar            token names (tok-* classes)
// The real grammar lives in specs/todotxt.abnf; nothing here validates, it only colors and places.
//
// UMD-lite export so the same file works as a <script> (window.TodoTxt) and as a Node import.
// Ref (UMD pattern): https://github.com/umdjs/umd
(function (root, factory) {
  const api = factory();
  if (typeof module === "object" && module.exports) module.exports = api;
  else root.TodoTxt = api;
})(typeof self !== "undefined" ? self : this, function () {
  const ISO = "\\d{4}-\\d{2}-\\d{2}";
  const DATE_RE = new RegExp(`^${ISO}$`);
  // `x` + completion date + optional creation date — same shape as lineInfo.ts's COMPLETED_LINE.
  const COMPLETED_RE = new RegExp(`^x ${ISO}(?: ${ISO})?(?: |$)`);
  // Slug grammar per plan §3.2.1 (lineInfo.ts REF_TAG).
  const REF_RE = /\bref:([a-z0-9][a-z0-9._-]*)/;
  const PRIORITY_RE = /^\(([A-Z])\)$/;
  const TAG_RE = /^([A-Za-z][\w-]*:)(\S+)$/;

  /** Advisory line-width hint, root todo 9 (apps/desktop/src/lib/todotxt/decorations.ts). */
  const LINE_LENGTH_HINT = 100;

  /** Offset where a completed line's struck description starts, or null (lineInfo.ts). */
  function completedInfo(text) {
    const m = COMPLETED_RE.exec(text);
    return m ? { descriptionStart: m[0].length } : null;
  }

  /** First `ref:<slug>` on the line, or null (lineInfo.ts). */
  function refSlug(text) {
    const m = REF_RE.exec(text);
    return m ? m[1] : null;
  }

  /** "A".."Z" of a leading `(X)` priority, else null. Completed lines have none. */
  function priorityOf(text) {
    const m = /^\(([A-Z])\)(?: |$)/.exec(text);
    return m ? m[1] : null;
  }

  /**
   * Splits one line into `{text, cls, from, to}` segments that cover the whole line, whitespace
   * included (cls "" = whitespace). Classes are the real app's token names: tok-completion-marker,
   * tok-priority, tok-date, tok-project, tok-context, tok-tag-key, tok-tag-value, tok-id-tag,
   * tok-text. Like the real grammar, marker/priority/date only count inside the line's leading run.
   */
  function tokenize(line) {
    const segs = [];
    const re = /(\s+)|(\S+)/g;
    let m;
    let leading = true; // still in the unbroken run of marker / priority / dates
    let wordIndex = 0;
    const push = (text, cls, from) => segs.push({ text, cls, from, to: from + text.length });
    while ((m = re.exec(line))) {
      if (m[1]) {
        push(m[1], "", m.index);
        continue;
      }
      const w = m[2];
      const at = m.index;
      if (leading && wordIndex === 0 && w === "x" && COMPLETED_RE.test(line)) {
        push(w, "tok-completion-marker", at);
      } else if (leading && PRIORITY_RE.test(w)) {
        push(w, "tok-priority", at);
      } else if (leading && DATE_RE.test(w)) {
        push(w, "tok-date", at);
      } else if (w[0] === "+" && w.length > 1) {
        leading = false;
        push(w, "tok-project", at);
      } else if (w[0] === "@" && w.length > 1) {
        leading = false;
        push(w, "tok-context", at);
      } else if (/^id:\S+$/.test(w)) {
        leading = false;
        push(w, "tok-id-tag", at);
      } else if (TAG_RE.test(w) && !/^[A-Za-z][\w-]*:\/\//.test(w)) {
        leading = false;
        const [, key, val] = TAG_RE.exec(w);
        push(key, "tok-tag-key", at);
        push(val, DATE_RE.test(val) ? "tok-date" : "tok-tag-value", at + key.length);
      } else {
        leading = false;
        push(w, "tok-text", at);
      }
      wordIndex++;
    }
    return segs;
  }

  /** Splits any segment that straddles `offset`, tagging the tail with `tailCls` (e.g. tx-long). */
  function markTail(segs, offset, tailCls) {
    const out = [];
    for (const s of segs) {
      if (s.to <= offset) out.push(s);
      else if (s.from >= offset) out.push({ ...s, cls: `${s.cls} ${tailCls}`.trim() });
      else {
        const k = offset - s.from;
        out.push({ text: s.text.slice(0, k), cls: s.cls, from: s.from, to: offset });
        out.push({ text: s.text.slice(k), cls: `${s.cls} ${tailCls}`.trim(), from: offset, to: s.to });
      }
    }
    return out;
  }

  /** Same marking for the struck description of a completed line. */
  function markStrike(segs, start) {
    return markTail(segs, start, "tx-strike");
  }

  /** Counts for status bars. A blank line is an entry but not a task (design §2.6). */
  function stats(doc) {
    const lines = doc.split("\n");
    let done = 0;
    let open = 0;
    for (const l of lines) {
      if (l.trim() === "") continue;
      if (completedInfo(l)) done++;
      else open++;
    }
    return { lines: lines.length, done, open, total: done + open };
  }

  // ---- search ----------------------------------------------------------------------------------

  /**
   * "desk (A) is:open" → {terms: ["desk", "(a)"], flags: {open: true}, active: true}.
   * Terms are AND-ed, case-insensitive substrings; `is:done` / `is:open` are the only operators.
   * A blank query is inactive (matches everything, highlights nothing).
   */
  function parseQuery(q) {
    const terms = [];
    const flags = {};
    for (const w of String(q || "").trim().toLowerCase().split(/\s+/).filter(Boolean)) {
      if (w === "is:done") flags.done = true;
      else if (w === "is:open") flags.open = true;
      else terms.push(w);
    }
    return { terms, flags, active: terms.length > 0 || !!flags.done || !!flags.open };
  }

  function matchesQuery(text, query) {
    if (!query.active) return true;
    if (query.flags.done && !completedInfo(text)) return false;
    if (query.flags.open && (completedInfo(text) || text.trim() === "")) return false;
    const t = text.toLowerCase();
    return query.terms.every((w) => t.includes(w));
  }

  /** Merged `{from, to}` ranges of every term occurrence in `text`. */
  function findRanges(text, query) {
    const t = text.toLowerCase();
    const raw = [];
    for (const w of query.terms) {
      for (let i = t.indexOf(w); i !== -1; i = t.indexOf(w, i + w.length)) raw.push({ from: i, to: i + w.length });
    }
    raw.sort((a, b) => a.from - b.from);
    const out = [];
    for (const r of raw) {
      const last = out[out.length - 1];
      if (last && r.from <= last.to) last.to = Math.max(last.to, r.to);
      else out.push({ ...r });
    }
    return out;
  }

  /** Splits segments at range edges and tags the inside with `cls` (the search hit). */
  function markRanges(segs, ranges, cls) {
    if (!ranges.length) return segs;
    const points = [...new Set(ranges.flatMap((r) => [r.from, r.to]))].sort((a, b) => a - b);
    const inside = (pos) => ranges.some((r) => pos >= r.from && pos < r.to);
    const out = [];
    for (const s of segs) {
      const cut = [s.from, ...points.filter((p) => p > s.from && p < s.to), s.to];
      for (let i = 0; i < cut.length - 1; i++) {
        const a = cut[i];
        const b = cut[i + 1];
        out.push({ text: s.text.slice(a - s.from, b - s.from), from: a, to: b, cls: inside(a) ? `${s.cls} ${cls}`.trim() : s.cls });
      }
    }
    return out;
  }

  // ---- task model (for the Universal view) -------------------------------------------------------

  /** A display-only read of one line: what a list row needs, not a parser (see lineInfo.ts). */
  function parseTask(text) {
    const done = completedInfo(text);
    const words = text.split(/\s+/).filter(Boolean);
    const tag = (k) => {
      const w = words.find((x) => x.startsWith(k + ":"));
      return w ? w.slice(k.length + 1) : null;
    };
    let body = text;
    if (done) body = text.slice(done.descriptionStart);
    else body = text.replace(/^\([A-Z]\) /, "").replace(new RegExp(`^${ISO} `), "");
    const pri = priorityOf(text) || (done && (text.match(/\bpri:([A-Z])\b/) || [])[1]) || null;
    return {
      done: !!done,
      pri,
      due: tag("due"),
      ref: refSlug(text),
      projects: words.filter((w) => w[0] === "+" && w.length > 1),
      contexts: words.filter((w) => w[0] === "@" && w.length > 1),
      body,
    };
  }

  /** Whole days from `from` to `to` (both YYYY-MM-DD). Negative = `to` is in the past. */
  function daysBetween(from, to) {
    const ms = (s) => Date.UTC(+s.slice(0, 4), +s.slice(5, 7) - 1, +s.slice(8, 10));
    return Math.round((ms(to) - ms(from)) / 86400000);
  }

  /** "overdue 2d" / "today" / "tomorrow" / "in 4d" / "Oct 15", with a tone for the badge. */
  function dueLabel(due, today) {
    if (!due || !DATE_RE.test(due)) return null;
    const d = daysBetween(today, due);
    if (d < 0) return { text: `overdue ${-d}d`, tone: "overdue", days: d };
    if (d === 0) return { text: "today", tone: "today", days: d };
    if (d === 1) return { text: "tomorrow", tone: "soon", days: d };
    if (d <= 7) return { text: `in ${d}d`, tone: "soon", days: d };
    const mon = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"][+due.slice(5, 7) - 1];
    return { text: `${mon} ${+due.slice(8, 10)}`, tone: "later", days: d };
  }

  /** `YYYY-MM-DD`, local zone (editPopoverLogic.ts localToday). */
  function localToday(now = new Date()) {
    const p = (n) => String(n).padStart(2, "0");
    return `${now.getFullYear()}-${p(now.getMonth() + 1)}-${p(now.getDate())}`;
  }

  // ---- chip / completion transforms (ported from editPopoverLogic.ts) --------------------------

  const COMPLETED_FULL = new RegExp(`^x (${ISO})(?: (${ISO}))?(?: (.*))?$`);
  const LEADING_DATE = new RegExp(`^(${ISO})(?: (.*))?$`);

  function stripPriority(raw) {
    const m = raw.match(/^\(([A-Z])\) ?/);
    return m ? { letter: m[1], rest: raw.slice(m[0].length) } : null;
  }

  /** `x` chip / gutter click: on → `x <today> [created] desc [pri:P]`, off → reverse. */
  function toggleComplete(raw, today) {
    const done = raw.match(COMPLETED_FULL);
    if (done) {
      const [, , created, description = ""] = done;
      const words = description ? description.split(" ") : [];
      let pri = null;
      const rest = words.filter((w) => {
        if (pri === null) {
          const m = w.match(/^pri:([A-Z])$/);
          if (m) {
            pri = m[1];
            return false;
          }
        }
        return true;
      });
      const parts = [];
      if (pri) parts.push(`(${pri})`);
      if (created) parts.push(created);
      if (rest.length) parts.push(rest.join(" "));
      return parts.join(" ");
    }
    const stripped = stripPriority(raw);
    const rest = stripped ? stripped.rest : raw;
    const dated = rest.match(LEADING_DATE);
    const created = dated ? dated[1] : null;
    const description = dated ? dated[2] ?? "" : rest;
    const parts = ["x", today];
    if (created) parts.push(created);
    if (description) parts.push(description);
    if (stripped) parts.push(`pri:${stripped.letter}`);
    return parts.join(" ");
  }

  /** Applies one chip at `caret` in one line → `{text, caret}` (editPopoverLogic.ts applyChip). */
  function applyChip(raw, caret, chip, today) {
    if (chip === "A" || chip === "B" || chip === "C") {
      const stripped = stripPriority(raw);
      const rest = stripped ? stripped.rest : raw;
      const oldPrefix = raw.length - rest.length;
      const removing = stripped && stripped.letter === chip;
      const text = removing ? rest : rest.length ? `(${chip}) ${rest}` : `(${chip})`;
      const delta = text.length - rest.length - oldPrefix;
      return { text, caret: Math.max(0, Math.min(text.length, caret + delta)) };
    }
    if (chip === "x") {
      const text = toggleComplete(raw, today);
      return { text, caret: text.length };
    }
    const before = raw.slice(0, caret);
    const after = raw.slice(caret);
    const atBoundary = before.length === 0 || /\s$/.test(before);
    const insert = atBoundary ? chip : ` ${chip}`;
    return { text: before + insert + after, caret: before.length + insert.length };
  }

  /**
   * Lenient stand-in for the popover's strict-mode hint (never blocks a save — design §2.3).
   * Returns a message or null. Only catches the obvious shapes; the real check is the wasm core.
   */
  function strictHint(line) {
    if (/^\([a-z]\)/.test(line)) return "Priority letters are uppercase: (A)–(Z).";
    if (/^\((?![A-Z]\))/.test(line)) return "A leading (…) must be a single priority letter.";
    if (/\b(?:due|t):(?!\d{4}-\d{2}-\d{2}\b)\S*/.test(line)) return "due: and t: take a date, YYYY-MM-DD.";
    return null;
  }

  return {
    LINE_LENGTH_HINT,
    completedInfo,
    refSlug,
    priorityOf,
    tokenize,
    markTail,
    markStrike,
    parseQuery,
    matchesQuery,
    findRanges,
    markRanges,
    parseTask,
    daysBetween,
    dueLabel,
    stats,
    localToday,
    toggleComplete,
    applyChip,
    strictHint,
  };
});
