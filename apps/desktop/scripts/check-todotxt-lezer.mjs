#!/usr/bin/env node
// check-todotxt-lezer.mjs — CI gate for the todo.txt Lezer grammar (tasks/desktop-lezer-grammar).
//
// Two checks, both required by tasks/desktop-lezer-grammar/notes.md's acceptance criteria:
//   1. Idempotence: running abnf-to-lezer.mjs twice produces byte-identical generated files.
//      (The CI job separately runs the generator once more and `git diff --exit-code`s the
//      checked-in files against a clean checkout, to catch drift from specs/todotxt.abnf too —
//      see the "lezer" job in .github/workflows/ci.yml. This check is the *idempotence* half.)
//   2. Corpus coverage: every line in every corpus/*.tokens.json (the oracles core-scanner-tokenize
//      owns) tokenizes, through the compiled todotxt.grammar, to the expected plan §3.1 token name.
//      A lenient/quirked line must still render fully as real spans (never an error node) — this
//      check fails loudly if one doesn't.
//
// No test runner is configured yet in apps/desktop/package.json (that's desktop-stack-mapping's
// job); this is a plain Node script, wired into CI directly, that asserts and exits non-zero.

import { readFileSync, readdirSync } from "node:fs";
import { execFileSync } from "node:child_process";
import { fileURLToPath } from "node:url";
import path from "node:path";

const here = path.dirname(fileURLToPath(import.meta.url));
const DESKTOP_ROOT = path.join(here, "..");
const REPO_ROOT = path.resolve(DESKTOP_ROOT, "..", "..");
const LANG_DIR = path.join(DESKTOP_ROOT, "src", "lang");
const CORPUS_DIR = path.join(REPO_ROOT, "corpus");
const GENERATED_FILES = ["todotxt.grammar", "todotxt.shapes.js", "todotxt.parser.js", "todotxt.parser.terms.js"];

let failures = 0;
const fail = (msg) => { failures++; console.error(`FAIL: ${msg}`); };

// ------------------------------------------------------------------------------------------
// 1. Idempotence: run the real generator (writing into the real src/lang), snapshot the four
//    generated files, run it again, and require byte-for-byte equality.
// ------------------------------------------------------------------------------------------
function checkRealIdempotence() {
  execFileSync(process.execPath, [path.join(DESKTOP_ROOT, "scripts", "abnf-to-lezer.mjs")], { stdio: "pipe" });
  const before = Object.fromEntries(GENERATED_FILES.map((f) => [f, readFileSync(path.join(LANG_DIR, f))]));
  execFileSync(process.execPath, [path.join(DESKTOP_ROOT, "scripts", "abnf-to-lezer.mjs")], { stdio: "pipe" });
  const after = Object.fromEntries(GENERATED_FILES.map((f) => [f, readFileSync(path.join(LANG_DIR, f))]));
  for (const f of GENERATED_FILES) {
    if (!before[f].equals(after[f])) fail(`${f} is not idempotent: a second run of abnf-to-lezer.mjs changed it`);
  }
  if (failures === 0) console.log(`idempotence: ${GENERATED_FILES.join(", ")} are byte-identical across two runs`);
}

// ------------------------------------------------------------------------------------------
// 2. Corpus coverage: every corpus/*.tokens.json line tokenizes to the expected §3.1 name.
// ------------------------------------------------------------------------------------------

// The corpus oracle (core-scanner-tokenize) has a richer kind set than this grammar's nine plan
// §3.1 token names. This maps oracle kinds down to what THIS grammar is expected to produce:
//   - CompletionDate/CreationDate both fold into `date` (§3.1 has one `date` name, not two).
//   - Url isn't one of the nine §3.1 names; the grammar has no URL detection, so it's `text`.
//   - Whitespace isn't a §3.1 name either; @skip removes it from the tree, so it's not compared.
//   - A TagValue whose text has the exact date shape is expected as `date`, not `tag-value` — see
//     abnf-to-lezer.mjs's file-level comment for why (due-tag/t-tag reuse the ABNF `date` rule).
const KIND_TO_TOKEN = {
  CompletionMarker: "completion-marker", CompletionDate: "date", CreationDate: "date",
  Priority: "priority", Project: "project", Context: "context", TagKey: "tag-key",
  TagValue: "tag-value", IdTag: "id-tag", Text: "text", Url: "text", Whitespace: null,
};
const DATE_RE = /^\d{4}-\d{2}-\d{2}$/;

const NODE_TO_TOKEN = {
  CompletionMarker: "completion-marker", Priority: "priority", Date: "date", Project: "project",
  Context: "context", TagKey: "tag-key", TagValue: "tag-value", IdTag: "id-tag", Text: "text",
};

// The corpus stores UTF-8 BYTE offsets (tokens.schema.json); Lezer/CodeMirror always work in JS
// UTF-16 code-unit offsets. Convert before comparing so multi-byte lines (CJK, emoji, accents)
// don't spuriously fail.
function byteOffsetToUtf16Index(raw) {
  const map = new Map([[0, 0]]);
  let byteLen = 0, utf16Len = 0;
  for (const ch of raw) { // iterates by code point; handles surrogate pairs correctly
    byteLen += Buffer.byteLength(ch, "utf8");
    utf16Len += ch.length; // 1, or 2 for an astral code point
    map.set(byteLen, utf16Len);
  }
  return (byteOffset) => map.get(byteOffset);
}

function expectedTokens(entry) {
  const toUtf16 = byteOffsetToUtf16Index(entry.raw);
  const out = [];
  for (const span of entry.spans) {
    if (span.kind === "Whitespace") continue;
    if (!(span.kind in KIND_TO_TOKEN)) throw new Error(`unknown corpus span kind ${JSON.stringify(span.kind)}`);
    let name = KIND_TO_TOKEN[span.kind];
    const start = toUtf16(span.start), end = toUtf16(span.end);
    if (start === undefined || end === undefined) throw new Error(`span [${span.start},${span.end}) isn't on a code-point boundary in ${JSON.stringify(entry.raw)}`);
    if (span.kind === "TagValue" && DATE_RE.test(entry.raw.slice(start, end))) name = "date";
    out.push({ name, start, end });
  }
  return out;
}

function actualTokens(parser, raw) {
  const tree = parser.parse(raw);
  const out = [];
  tree.iterate({ enter: (n) => { const name = NODE_TO_TOKEN[n.type.name]; if (name) out.push({ name, start: n.from, end: n.to }); } });
  out.sort((a, b) => a.start - b.start || a.end - b.end);
  return out;
}

async function checkCorpusCoverage() {
  const { parser } = await import(path.join(LANG_DIR, "todotxt.parser.js"));
  const files = readdirSync(CORPUS_DIR).filter((f) => f.endsWith(".tokens.json")).sort();
  if (!files.length) throw new Error(`no corpus/*.tokens.json files found under ${CORPUS_DIR}`);
  let lines = 0;
  for (const file of files) {
    const entries = JSON.parse(readFileSync(path.join(CORPUS_DIR, file), "utf8"));
    for (const entry of entries) {
      lines++;
      let expected, actual;
      try {
        expected = expectedTokens(entry);
        actual = actualTokens(parser, entry.raw);
      } catch (err) {
        fail(`${file} ${JSON.stringify(entry.raw)}: ${err.message}`);
        continue;
      }
      if (JSON.stringify(expected) !== JSON.stringify(actual)) {
        fail(
          `${file} ${JSON.stringify(entry.raw)}\n` +
          `     expected: ${JSON.stringify(expected)}\n` +
          `     actual:   ${JSON.stringify(actual)}`,
        );
      }
    }
  }
  if (failures === 0) console.log(`corpus coverage: ${lines} lines across ${files.length} files all tokenized to the expected §3.1 name`);
}

checkRealIdempotence();
await checkCorpusCoverage();

if (failures > 0) {
  console.error(`\n${failures} check(s) failed.`);
  process.exit(1);
}
console.log("\nall checks passed.");
