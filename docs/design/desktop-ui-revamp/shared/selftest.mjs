// Self-asserting check for the mockups' pure logic. No browser, no network:
//   node apps/desktop/design-mockups/shared/selftest.mjs
// Exits non-zero on the first failed assertion.
// Ref (node:assert strict): https://nodejs.org/api/assert.html#strict-assertion-mode
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import vm from "node:vm";

// apps/desktop/package.json is `"type": "module"`, so Node would treat the browser-style UMD
// files as ESM and `module` would be undefined. Evaluate them in a sandbox that provides `module`
// instead — the exact shape the UMD wrapper looks for.
// Ref (vm.runInNewContext): https://nodejs.org/api/vm.html#vmruninnewcontextcode-contextobject-options
function load(file) {
  const sandbox = { module: { exports: {} } };
  vm.runInNewContext(readFileSync(new URL(file, import.meta.url), "utf8"), sandbox, { filename: file });
  return sandbox.module.exports;
}
const T = load("./todotxt.js");
const D = load("./data.js");

// Spread first: arrays built inside the vm sandbox are a different realm's Array, which
// deepStrictEqual rejects even when the contents match.
const classes = (line) => [...T.tokenize(line)].filter((s) => s.cls).map((s) => `${s.cls}:${s.text}`);

// Tokens: leading run vs. mid-line, tags split into key + value, date-valued tags become dates.
assert.deepEqual(classes("(A) 2026-09-14 plan +work @desk due:2026-09-26 ref:q4"), [
  "tok-priority:(A)",
  "tok-date:2026-09-14",
  "tok-text:plan",
  "tok-project:+work",
  "tok-context:@desk",
  "tok-tag-key:due:",
  "tok-date:2026-09-26",
  "tok-tag-key:ref:",
  "tok-tag-value:q4",
]);
assert.deepEqual(classes("x 2026-09-18 2026-09-11 done it"), [
  "tok-completion-marker:x",
  "tok-date:2026-09-18",
  "tok-date:2026-09-11",
  "tok-text:done",
  "tok-text:it",
]);
// A priority-looking word mid-line is plain text; a URL is not a tag.
assert.deepEqual(classes("call (A) https://example.com"), ["tok-text:call", "tok-text:(A)", "tok-text:https://example.com"]);
// Segments cover the line exactly (whitespace included), so the mirror can never drift.
for (const line of D.DOC.split("\n")) {
  assert.equal(T.tokenize(line).map((s) => s.text).join(""), line);
}

// Completion + refs (lineInfo.ts parity).
assert.equal(T.completedInfo("x 2026-09-18 2026-09-11 hi").descriptionStart, "x 2026-09-18 2026-09-11 ".length);
assert.equal(T.completedInfo("x no date"), null);
assert.equal(T.refSlug("a ref:q4-roadmap b"), "q4-roadmap");

// The sample doc: exactly one line past the 100-char hint; every ref resolves; ends with a blank.
const lines = D.DOC.split("\n");
assert.equal(lines.filter((l) => l.length > T.LINE_LENGTH_HINT).length, 1);
for (const l of lines) {
  const slug = T.refSlug(l);
  if (slug) assert.ok(D.REFS[slug] && D.DETAILS[slug], `dangling ref ${slug}`);
}
assert.equal(lines[lines.length - 1], "");
const s = T.stats(D.DOC);
assert.deepEqual([s.open, s.done], [9, 3]);
// The sub-list's own numbers must match the REFS pill the main list shows.
const sub = T.stats(D.DETAILS["q4-roadmap"].list);
assert.deepEqual([sub.done, sub.total], [D.REFS["q4-roadmap"].done, D.REFS["q4-roadmap"].total]);

// Chips (editPopoverLogic.ts parity): complete, uncomplete, priority swap/remove, no double space.
const today = "2026-09-19";
assert.equal(T.toggleComplete("(B) 2026-09-17 buy milk", today), "x 2026-09-19 2026-09-17 buy milk pri:B");
assert.equal(T.toggleComplete("x 2026-09-19 2026-09-17 buy milk pri:B", today), "(B) 2026-09-17 buy milk");
assert.equal(T.applyChip("(B) buy milk", 0, "A", today).text, "(A) buy milk");
assert.equal(T.applyChip("(A) buy milk", 4, "A", today).text, "buy milk");
assert.equal(T.applyChip("buy milk", 8, "+", today).text, "buy milk +");
assert.equal(T.applyChip("buy milk ", 9, "@", today).text, "buy milk @");
assert.equal(T.strictHint("(a) nope"), "Priority letters are uppercase: (A)–(Z).");
assert.equal(T.strictHint("(A) ok due:2026-09-26"), null);

// Search (c2): AND-ed terms, is:done / is:open, merged hit ranges, hits split token spans exactly.
const q = T.parseQuery("Desk is:open");
assert.deepEqual([...q.terms], ["desk"]);
assert.equal(T.matchesQuery("(A) plan @desk", q), true);
assert.equal(T.matchesQuery("x 2026-09-18 2026-09-11 plan @desk", q), false);
assert.equal(T.matchesQuery("", T.parseQuery("is:open")), false);
assert.equal(T.parseQuery("   ").active, false);
assert.deepEqual(JSON.parse(JSON.stringify(T.findRanges("plan @desk +desk", T.parseQuery("desk")))), [{ from: 6, to: 10 }, { from: 12, to: 16 }]);
const marked = T.markRanges(T.tokenize("plan @desk"), T.findRanges("plan @desk", T.parseQuery("des")), "tx-hit");
assert.equal([...marked].map((s) => s.text).join(""), "plan @desk");
assert.deepEqual([...marked].filter((s) => s.cls.includes("tx-hit")).map((s) => s.text), ["des"]);

// Task model + due labels (Universal).
const pt = T.parseTask("(A) 2026-09-14 plan @desk +work due:2026-09-26 ref:q4");
assert.deepEqual([pt.pri, pt.due, pt.ref, pt.done], ["A", "2026-09-26", "q4", false]);
assert.equal(T.parseTask("x 2026-09-18 2026-09-11 finish it pri:B").pri, "B");
assert.equal(T.dueLabel("2026-09-18", "2026-09-19").tone, "overdue");
assert.equal(T.dueLabel("2026-09-19", "2026-09-19").text, "today");
assert.equal(T.dueLabel("2026-10-15", "2026-09-19").text, "Oct 15");
// Every workspace sample resolves its refs, and the docs' open counts match the workspace list.
for (const w of D.WORKSPACES) {
  const doc = w.name === "notes" ? D.DOC : D.WS_DOCS[w.name];
  assert.equal(T.stats(doc).open, w.open, `${w.name} open count`);
  for (const l of doc.split("\n")) {
    const slug = T.refSlug(l);
    if (slug) assert.ok(D.WS_REFS[w.name][slug] && D.DETAILS[slug], `dangling ref ${slug} in ${w.name}`);
  }
}

console.log("selftest ok");
