// todotxt.tokens.js — hand-written support code for todotxt.grammar.
//
// NOT generated: this is the one piece of the todo.txt Lezer language pack that can't be a
// declarative `@tokens` regex, because a few of the plan §3.1 token names need lookahead or
// parse-state context that Lezer's tokenizer DSL can't express on its own:
//   https://lezer.codemirror.net/docs/ref/#lr.ExternalTokenizer
//   https://lezer.codemirror.net/docs/ref/#lr.ContextTracker
//
//  - CompletionMarker/Priority/(bare) Date are only highlighted while still in a line's leading
//    run (see todotxtContext below) — otherwise "X 2026-09-11 not done" (capital X, an invalid
//    marker) would highlight "2026-09-11" as a date even though it's just the second word of a
//    plain-text line. That needs to know how many *prior* tokens on this line were recognised,
//    which only a stateful context (not a plain regex) can track.
//  - TagKey only exists when a "key:" is followed by at least one non-space character (ABNF
//    `tag = key ":" 1*NONSP` — note the `1*`): "note: buy milk" has no tag, the whole "note:" is
//    `text`, which needs one character of lookahead *past* the colon.
//  - A "scheme://" URL (e.g. "https://example.com") is deliberately never split into a tag, even
//    though it contains a colon — same rationale real todo.txt tools use.
//  - IdTag is `id:` plus a whole valid ULID, checked and emitted as a single token (notes.md:
//    "whole id:<ulid>, hidden by the main view"), which again needs lookahead past the ":".
//
// All the concrete *shapes* being matched (which characters, how many digits, what the "id:"
// prefix literally is) live in the generated todotxt.shapes.js, mechanically derived from
// specs/todotxt.abnf by abnf-to-lezer.mjs — this file only contains generic matching logic, no
// magic numbers, so it never has to change when a shape in the ABNF does.
import { ExternalTokenizer, ContextTracker } from "@lezer/lr";
import {
  CompletionMarker, Priority, Date, TagKey, TagValue, IdTag, Space, Newline,
} from "./todotxt.parser.terms.js";
import {
  COMPLETION_MARKER, PRIORITY_RANGE, DATE_SHAPE, ULID_PREFIX, ULID_LENGTH, ULID_RANGES, KEY_RANGES,
  NONSP_RANGES,
} from "./todotxt.shapes.js";

/**
 * @param {number} cp
 * @param {ReadonlyArray<ReadonlyArray<number>>} ranges pairs of [lo, hi] codepoints
 * @returns {boolean}
 */
function inRanges(cp, ranges) {
  for (const [lo, hi] of ranges) if (cp >= lo && cp <= hi) return true;
  return false;
}
/** @param {number} c */
const isHSpace = (c) => c === 32 || c === 9; // space, tab — NOT newline
/** @param {number} c */
const isNL = (c) => c === 10 || c === 13;
/** @param {number} c */
const isNonSp = (c) => inRanges(c, NONSP_RANGES); // ABNF NONSP: anything except whitespace/control
/** @param {number} c */
const isKeyChar = (c) => inRanges(c, KEY_RANGES);
/** @param {number} c */
const isDigit = (c) => c >= 0x30 && c <= 0x39;
/** @param {number} c */
const isPriorityLetter = (c) => c >= PRIORITY_RANGE[0] && c <= PRIORITY_RANGE[1];

/**
 * How many characters (code units) `input.peek` needs to walk to confirm a match, without
 * consuming anything until the whole shape is confirmed (InputStream.peek never advances the
 * stream — https://lezer.codemirror.net/docs/ref/#lr.InputStream.peek).
 * @param {import("@lezer/lr").InputStream} input
 * @param {number} offset
 * @param {string} text
 * @returns {number} the offset just past `text`, or -1 if it doesn't match at `offset`
 */
function matchLiteral(input, offset, text) {
  for (let i = 0; i < text.length; i++) if (input.peek(offset + i) !== text.codePointAt(i)) return -1;
  return offset + text.length;
}
/**
 * @param {import("@lezer/lr").InputStream} input
 * @param {number} offset
 * @returns {number} the offset just past the date, or -1 if it doesn't match at `offset`
 */
function matchDateShape(input, offset) {
  let p = offset;
  for (const group of DATE_SHAPE) {
    if (typeof group.literal === "string") {
      const next = matchLiteral(input, p, group.literal);
      if (next < 0) return -1;
      p = next;
    } else if (typeof group.digits === "number") {
      for (let d = 0; d < group.digits; d++) {
        if (!isDigit(input.peek(p))) return -1;
        p++;
      }
    }
  }
  return p;
}
/**
 * @param {import("@lezer/lr").InputStream} input
 * @param {number} offset
 * @returns {number} the offset just past the ulid, or -1 if it doesn't match at `offset`
 */
function matchUlid(input, offset) {
  const afterPrefix = matchLiteral(input, offset, ULID_PREFIX);
  if (afterPrefix < 0) return -1;
  let p = afterPrefix;
  for (let i = 0; i < ULID_LENGTH; i++) {
    if (!inRanges(input.peek(p), ULID_RANGES)) return -1;
    p++;
  }
  return isNonSp(input.peek(p)) ? -1 : p; // the ulid must be the whole word, nothing else attached
}

// leading-run state: 0 = start of line (CompletionMarker still allowed), 1 = still in the leading
// run but past word 0 (Priority/Date still allowed, CompletionMarker no longer), 2 = broken (a
// non-special word has appeared; rest of the line is never CompletionMarker/Priority/Date).
export const todotxtContext = new ContextTracker({
  start: 0,
  shift(context, term) {
    if (term === Newline) return 0;
    if (term === Space) return context;
    if (term === CompletionMarker || term === Priority || term === Date) return 1;
    return 2;
  },
});

export const todotxtTokens = new ExternalTokenizer(
  (input, stack) => {
    // Horizontal whitespace and newlines are matched here (not a plain @tokens rule) purely so
    // their term ids are visible to todotxtContext above — see the file-level comment in
    // todotxt.grammar. They carry no @lezer/highlight tag; @skip drops them from the tree.
    if (isHSpace(input.peek(0))) {
      let i = 0;
      while (isHSpace(input.peek(i))) i++;
      input.acceptToken(Space, i);
      return;
    }
    if (input.peek(0) === 13 && input.peek(1) === 10) { input.acceptToken(Newline, 2); return; }
    if (isNL(input.peek(0))) { input.acceptToken(Newline, 1); return; }

    // id:<ulid> is always a single IdTag token, anywhere on the line (plan §3.1: "hidden by the
    // main view" — the main view decorates the whole span, so it must never be split further).
    const ulidEnd = matchUlid(input, 0);
    if (ulidEnd > 0) { input.acceptToken(IdTag, ulidEnd); return; }

    const leading = stack.context;

    // CompletionMarker: only the literal "x" (case-sensitive, RFC 7405 %s"x"), only as word 0,
    // and only when it's the whole word (an "x" glued to more text is just a word, e.g. "x-ray").
    if (leading === 0) {
      const markerEnd = matchLiteral(input, 0, COMPLETION_MARKER);
      if (markerEnd > 0 && !isNonSp(input.peek(markerEnd))) { input.acceptToken(CompletionMarker, markerEnd); return; }
    }
    // Priority "(A)".."(Z)": anywhere in the still-unbroken leading run (real files interleave it
    // with the completion/creation dates in any order — design §2.4's lenient quirks).
    if (leading <= 1 && input.peek(0) === 0x28 /* ( */ && isPriorityLetter(input.peek(1)) && input.peek(2) === 0x29 /* ) */
        && !isNonSp(input.peek(3))) {
      input.acceptToken(Priority, 3);
      return;
    }
    // Bare date (completion date or creation date), same leading-run rule as Priority.
    if (leading <= 1) {
      const dateEnd = matchDateShape(input, 0);
      if (dateEnd > 0 && !isNonSp(input.peek(dateEnd))) { input.acceptToken(Date, dateEnd); return; }
    }

    // Right after a shifted TagKey, the grammar (`Tag { TagKey (Date | TagValue) }`) only ever
    // wants a Date or a TagValue next — Stack.canShift lets us ask the parser, rather than
    // guessing from position, which restricts this branch to exactly that slot (see
    // https://lezer.codemirror.net/docs/ref/#lr.Stack.canShift). due-tag/t-tag reuse the ABNF
    // `date` rule for their value, so prefer Date whenever the whole value has that shape.
    if (stack.canShift(TagValue)) {
      const dateEnd = matchDateShape(input, 0);
      if (dateEnd > 0 && !isNonSp(input.peek(dateEnd))) { input.acceptToken(Date, dateEnd); return; }
      let i = 0;
      while (isNonSp(input.peek(i))) i++;
      if (i > 0) input.acceptToken(TagValue, i);
      return;
    }

    // key ":" 1*NONSP -> TagKey (colon included), but only when a value actually follows (else the
    // whole run stays `text`, e.g. "note:") and never for a "scheme://" URL.
    if (isKeyChar(input.peek(0))) {
      let i = 0;
      while (isKeyChar(input.peek(i))) i++;
      const isUrl = input.peek(i) === 0x3a && input.peek(i + 1) === 0x2f && input.peek(i + 2) === 0x2f; // "://"
      if (!isUrl && input.peek(i) === 0x3a && isNonSp(input.peek(i + 1))) input.acceptToken(TagKey, i + 1);
    }
  },
  { contextual: true },
);
