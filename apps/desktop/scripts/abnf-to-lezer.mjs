#!/usr/bin/env node
// abnf-to-lezer.mjs — generates the Lezer grammar CodeMirror 6 uses to highlight todo.txt files.
//
// Reads the NORMATIVE, frozen specs/todotxt.abnf (RFC 5234 ABNF, RFC 7405 %s"…" case-sensitive
// strings: https://www.rfc-editor.org/rfc/rfc5234, https://www.rfc-editor.org/rfc/rfc7405) and
// writes apps/desktop/src/lang/todotxt.grammar plus its compiled parser. This file is the
// mechanical mirror described in tasks/desktop-lezer-grammar/notes.md: the ABNF's header already
// names "the hand-written parser, the test-only generated parser, and M7's Lezer grammar" as the
// three things that must derive from specs/todotxt.abnf, so a hand-written .grammar would drift
// from it exactly like a second hand parser would. Run this script + `git diff --exit-code` is
// the fence (see the `lezer` CI job in .github/workflows/ci.yml).
//
// What's mechanically derived from the ABNF vs. what's fixed policy:
//   - The individual rule *shapes* (how many digits in a date, which hex ranges make a ulid, the
//     priority letter range, the literal "x"/"id:" prefixes, the key/NONSP character ranges) are
//     parsed out of specs/todotxt.abnf below — change a range there and this regenerates matching
//     Lezer/tokenizer code, no hand edit needed. That's TOKEN_SHAPES, written to todotxt.shapes.js.
//   - WHICH of the plan §3.1 semantic token names each rule maps to (completion-marker / priority /
//     date / project / context / tag-key / tag-value / id-tag / text) is a fixed product decision
//     given verbatim by tasks/desktop-lezer-grammar/notes.md's mapping table, not something to
//     infer from the grammar text — so that correspondence, and the tag-key/tag-value split, the
//     id-tag single-token special case, and the "lenient token soup" grammar shape, are authored
//     directly in the template below (notes.md's own design sketch does the same: it hardcodes
//     `TOKEN = {…}` rather than deriving it).
//
// Design notes (tasks/desktop-lezer-grammar/notes.md):
//   - CodeMirror renders LENIENT, real-world files (design §2.3: lenient is read-only), not just
//     strictly-valid ones, so this grammar is a flat, mostly position-independent "token soup"
//     rather than a re-implementation of the strict incomplete/completed ABNF alternatives. Only
//     `completion-marker` (word 0 only) and `priority`/`date` (anywhere in the still-unbroken
//     leading run of a line) are position-sensitive, tracked via a small @lezer/lr ContextTracker
//     in the hand-written todotxt.tokens.js. Everything else (project/context/tags/id-tag) is
//     recognised anywhere on the line. `text` is the total fallback: every byte tokenizes as
//     *something*, so a quirked/lenient line renders, never crashes (see check-todotxt-lezer.mjs).
//   - A `key:value`-shaped word only becomes a tag if at least one non-space character follows the
//     colon (ABNF: `tag = key ":" 1*NONSP`, note the `1*`); otherwise (e.g. "note: ") the whole run
//     is `text`. A `scheme://` URL is never treated as a tag either, even though it contains a
//     colon (real todo.txt tooling convention; also avoids "https" + "//example.com" nonsense).
//   - `due:`/`t:` tag values reuse the ABNF `date` rule (due-tag/t-tag), so — beyond notes.md's
//     literal wording — this generator colors ANY tag value that has the exact date shape as
//     `date` rather than `tag-value`, regardless of the key name. Simpler to implement (one
//     tokenizer rule instead of key-name-conditioned lookahead) and a strict superset of what
//     notes.md asks for; flagged here since it's the one place this generator goes beyond a literal
//     rule-by-rule reading of the mapping table.
//
// Lezer grammar file format: https://lezer.codemirror.net/docs/guide/#writing-a-grammar

import { readFileSync, writeFileSync, mkdirSync } from "node:fs";
import { fileURLToPath } from "node:url";
import path from "node:path";
import { buildParserFile } from "@lezer/generator";

const here = path.dirname(fileURLToPath(import.meta.url));
const REPO_ROOT = path.resolve(here, "..", "..", "..");
const ABNF_PATH = path.join(REPO_ROOT, "specs", "todotxt.abnf");
const LANG_DIR = path.join(here, "..", "src", "lang");

const GENERATED_BANNER =
  "// GENERATED FILE — do not hand-edit.\n" +
  "// Produced by apps/desktop/scripts/abnf-to-lezer.mjs from specs/todotxt.abnf.\n" +
  "// Regenerate: node apps/desktop/scripts/abnf-to-lezer.mjs\n";

// ============================================================================================
// 1. A tiny ABNF reader — just enough of RFC 5234/7405 to read the handful of rule shapes this
//    generator needs (char ranges, digit-run/literal sequences, exact-count repetition, %s"…"
//    literals). It is not a general ABNF-to-anything compiler; specs/todotxt.abnf's own header
//    fixes which rules exist, and this generator hardcodes which of those rules it reads.
// ============================================================================================

function stripComment(line) {
  // ';' starts a comment unless it's inside a "…" string literal.
  let inStr = false;
  for (let i = 0; i < line.length; i++) {
    if (line[i] === '"') inStr = !inStr;
    else if (line[i] === ";" && !inStr) return line.slice(0, i);
  }
  return line;
}

function readAbnfRules(text) {
  const rules = new Map();
  let current = null;
  for (const rawLine of text.split(/\r\n|\n/)) {
    const line = stripComment(rawLine);
    const def = line.match(/^([A-Za-z][A-Za-z0-9-]*)\s*=\s*(.*)$/);
    if (def) {
      current = def[1];
      rules.set(current, def[2].trim());
    } else if (current && /\S/.test(line) && /^\s/.test(rawLine)) {
      // indented continuation of the previous rule's body
      rules.set(current, `${rules.get(current)} ${line.trim()}`.trim());
    }
  }
  return rules;
}

function mergeRanges(ranges) {
  const sorted = [...ranges].sort((a, b) => a[0] - b[0]);
  const out = [];
  for (const [lo, hi] of sorted) {
    const last = out[out.length - 1];
    if (last && lo <= last[1] + 1) last[1] = Math.max(last[1], hi);
    else out.push([lo, hi]);
  }
  return out;
}

// Parses a "/"-separated list of `%xHH`, `%xHH-HH`, or the core rule `DIGIT`, optionally wrapped
// in one pair of parens (as in `1*(...)`'s body), into merged [lo, hi] codepoint ranges.
function parseCharAlt(rhs) {
  const body = rhs.trim().replace(/^\((.*)\)$/, "$1");
  const ranges = body.split("/").map((part) => {
    part = part.trim();
    if (part === "DIGIT") return [0x30, 0x39]; // RFC 5234 Appendix B.1
    const m = part.match(/^%x([0-9A-Fa-f]+)(?:-([0-9A-Fa-f]+))?$/);
    if (!m) throw new Error(`abnf-to-lezer: can't read char alternative ${JSON.stringify(part)} in ${JSON.stringify(rhs)}`);
    const lo = parseInt(m[1], 16);
    return [lo, m[2] ? parseInt(m[2], 16) : lo];
  });
  return mergeRanges(ranges);
}

function rule(rules, name) {
  const rhs = rules.get(name);
  if (!rhs) throw new Error(`abnf-to-lezer: specs/todotxt.abnf has no rule named ${JSON.stringify(name)}`);
  return rhs;
}

// `priority = "(" %x41-5A ")"` -> [0x41, 0x5A]
function extractPriorityRange(rules) {
  const m = rule(rules, "priority").match(/%x([0-9A-Fa-f]+)-([0-9A-Fa-f]+)/);
  if (!m) throw new Error("abnf-to-lezer: couldn't find priority's letter range in specs/todotxt.abnf");
  return [parseInt(m[1], 16), parseInt(m[2], 16)];
}

// `date = 4DIGIT "-" 2DIGIT "-" 2DIGIT` -> [{digits:4},{literal:"-"},{digits:2},{literal:"-"},{digits:2}]
function extractDateShape(rules) {
  const rhs = rule(rules, "date");
  const groups = [];
  const re = /(\d+)DIGIT|"([^"]*)"/g;
  let m;
  while ((m = re.exec(rhs))) groups.push(m[1] ? { digits: Number(m[1]) } : { literal: m[2] });
  if (!groups.length) throw new Error("abnf-to-lezer: couldn't read date's shape in specs/todotxt.abnf");
  return groups;
}

// `ulid = 26(DIGIT / %x41-48 / ...)` -> { length: 26, ranges: [...] }
function extractCountedCharset(rules, name) {
  const rhs = rule(rules, name);
  const m = rhs.match(/^(\d+)\((.*)\)$/);
  if (!m) throw new Error(`abnf-to-lezer: couldn't read ${name}'s repeat count/charset in specs/todotxt.abnf`);
  return { length: Number(m[1]), ranges: parseCharAlt(m[2]) };
}

// `key = 1*(%x21-39 / %x3B-7E / %x80-10FFFF)` / `NONSP = %x21-7E / %x80-10FFFF` -> ranges
function extractOneOrMoreCharset(rules, name) {
  const rhs = rule(rules, name).replace(/^1\*/, "");
  return parseCharAlt(rhs);
}

// `%s"…"` literal prefix, e.g. `completed`'s leading %s"x", or id-tag/due-tag/t-tag's %s"key:".
function extractLiteralPrefix(rules, name) {
  const m = rule(rules, name).match(/%s"([^"]*)"/);
  if (!m) throw new Error(`abnf-to-lezer: couldn't find a %s"…" literal in ${name}'s rule body`);
  return m[1];
}

// ============================================================================================
// 2. Read specs/todotxt.abnf and derive the shapes the grammar + hand-written tokenizer need.
// ============================================================================================

const abnfText = readFileSync(ABNF_PATH, "utf8");
const rules = readAbnfRules(abnfText);

const shapes = {
  completionMarker: extractLiteralPrefix(rules, "completed"), // "x"
  priorityRange: extractPriorityRange(rules), // [0x41, 0x5A]
  dateShape: extractDateShape(rules), // digit-run/literal sequence
  ulid: { prefix: extractLiteralPrefix(rules, "id-tag"), ...extractCountedCharset(rules, "ulid") },
  keyRanges: extractOneOrMoreCharset(rules, "key"), // already excludes ':' and space per the ABNF
  nonSpRanges: extractOneOrMoreCharset(rules, "NONSP"),
};

// ============================================================================================
// 3. Render a Lezer character-class literal ($[...]) from codepoint ranges.
//    https://lezer.codemirror.net/docs/guide/#writing-a-grammar
// ============================================================================================

function rangeToLezerSet(ranges) {
  const esc = (cp) => {
    if (cp === 0x5d || cp === 0x5c || cp === 0x5e || cp === 0x2d) return `\\u{${cp.toString(16)}}`; // ] \ ^ -
    if (cp >= 0x21 && cp <= 0x7e) return String.fromCodePoint(cp); // printable ASCII, literal
    return `\\u{${cp.toString(16)}}`;
  };
  const body = ranges.map(([lo, hi]) => (lo === hi ? esc(lo) : `${esc(lo)}-${esc(hi)}`)).join("");
  return `$[${body}]`;
}

// ============================================================================================
// 4. Emit apps/desktop/src/lang/todotxt.shapes.js — the plain-data half of the mirror: generic
//    shape constants the hand-written external tokenizer (todotxt.tokens.js) matches against, so
//    that file contains no magic numbers of its own (a changed ABNF range regenerates this without
//    touching the tokenizer's logic).
// ============================================================================================

function renderShapesModule(s) {
  return (
    GENERATED_BANNER +
    "\n" +
    `export const COMPLETION_MARKER = ${JSON.stringify(s.completionMarker)};\n` +
    `export const PRIORITY_RANGE = ${JSON.stringify(s.priorityRange)};\n` +
    `export const DATE_SHAPE = ${JSON.stringify(s.dateShape)};\n` +
    `export const ULID_PREFIX = ${JSON.stringify(s.ulid.prefix)};\n` +
    `export const ULID_LENGTH = ${JSON.stringify(s.ulid.length)};\n` +
    `export const ULID_RANGES = ${JSON.stringify(s.ulid.ranges)};\n` +
    `export const KEY_RANGES = ${JSON.stringify(s.keyRanges)}; // excludes ':' and space already (see specs/todotxt.abnf's key rule)\n` +
    `export const NONSP_RANGES = ${JSON.stringify(s.nonSpRanges)};\n`
  );
}

// ============================================================================================
// 5. Emit apps/desktop/src/lang/todotxt.grammar.
//
//    Node names are PascalCase (Lezer identifiers can't contain '-'); the kebab-case §3.1 name
//    each maps to is TOKEN_NAME_BY_NODE in todotxtLanguage.ts. Nine grammar nodes cover the nine
//    §3.1 names 1:1, except `tag` splits into two (TagKey, TagValue) per notes.md's design sketch.
//
//    CompletionMarker/Priority/Date (bare) and the TagKey/TagValue split and IdTag all need
//    lookahead/context the declarative @tokens block can't express (see the file-level comment),
//    so they're `@external tokens` resolved by the hand-written todotxt.tokens.js. Project/Context/
//    Text stay plain @tokens — they're recognised anywhere on the line, no lookahead needed.
// ============================================================================================

function renderGrammar(s) {
  return `// GENERATED FILE — do not hand-edit. See apps/desktop/scripts/abnf-to-lezer.mjs.
// Lezer grammar for todo.txt (CodeMirror 6 highlighting only — NOT the strict write grammar;
// specs/todotxt.abnf + crates/txtodo-core own strict validation). Token names mirror plan §3.1:
// completion-marker, priority, date, project, context, tag-key, tag-value, id-tag, text.
// Grammar guide: https://lezer.codemirror.net/docs/guide/#writing-a-grammar

@top File { item* }

// Tracks whether we're still in the still-unbroken leading run of a line (word 0 for
// CompletionMarker, any position up to the first non-special word for Priority/Date) — see
// todotxt.tokens.js. Also resolves TagKey/TagValue/IdTag, which need lookahead a plain @tokens
// regex can't express (does ":" have a value after it? is it a "scheme://" URL instead of a tag?).
@context todotxtContext from "./todotxt.tokens.js"
@external tokens todotxtTokens from "./todotxt.tokens.js" {
  CompletionMarker, Priority, Date, TagKey, TagValue, IdTag, Space, Newline
}

@tokens {
  // "+"/"@" are recognised anywhere on the line (unlike CompletionMarker/Priority/Date, which are
  // only recognised in a line's still-unbroken leading run — see todotxt.tokens.js).
  Project { "+" NonSp+ }
  Context { "@" NonSp+ }
  NonSp { ${rangeToLezerSet(s.nonSpRanges)} }
  Text { NonSp+ }
  @precedence { Project, Context, Text }
}

@skip { Space | Newline }

// tag = key ":" 1*NONSP -> tag-key ":" tag-value (notes.md: "split the key and the value"; the
// generated TagKey token includes the ":" itself). due-tag/t-tag reuse the 'date' rule for their
// value, so todotxt.tokens.js also prefers Date here whenever a tag's value has that exact shape.
Tag { TagKey (Date | TagValue) }

item { CompletionMarker | Priority | Date | Project | Context | IdTag | Tag | Text }
`;
}

// ============================================================================================
// 6. Write everything. The .grammar and .shapes.js are plain generated text (byte-identical on
//    every run given the same specs/todotxt.abnf — that's the idempotence the CI gate and
//    check-todotxt-lezer.mjs both check). The compiled parser is built ahead-of-time here with
//    @lezer/generator's buildParserFile, so the app itself only needs the @lezer/lr runtime
//    (https://lezer.codemirror.net/docs/guide/#writing-a-grammar) — no grammar text is parsed at
//    app startup.
// ============================================================================================

mkdirSync(LANG_DIR, { recursive: true });

const grammarText = renderGrammar(shapes);
writeFileSync(path.join(LANG_DIR, "todotxt.grammar"), grammarText);
writeFileSync(path.join(LANG_DIR, "todotxt.shapes.js"), renderShapesModule(shapes));

const { parser, terms } = buildParserFile(grammarText, {
  fileName: "todotxt.grammar",
  exportName: "parser",
});
writeFileSync(path.join(LANG_DIR, "todotxt.parser.js"), GENERATED_BANNER + "\n" + parser);
writeFileSync(path.join(LANG_DIR, "todotxt.parser.terms.js"), GENERATED_BANNER + "\n" + terms);

console.log("abnf-to-lezer: wrote todotxt.grammar, todotxt.shapes.js, todotxt.parser.js, todotxt.parser.terms.js");
