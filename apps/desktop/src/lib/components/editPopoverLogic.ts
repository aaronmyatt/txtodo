// Pure logic for `EditPopover.svelte` — no DOM, no Tauri, no CodeMirror, so it is plain-function
// testable (tasks/desktop-edit-popover/notes.md, plan §3.2). Grammar reference:
// `specs/todotxt.abnf` (`priority`, `completed`/`incomplete`, `pri-tag`).

/** Token chips per plan §3.2 / notes.md's `Chip` union — the exact nine the popover renders. */
export type Chip = "A" | "B" | "C" | "+" | "@" | "due:" | "t:" | "rec:" | "x";

/** Result of a text transform that also has to move the caret along with the edit. */
export interface ChipResult {
	text: string;
	caret: number;
}

const DATE_SRC = "\\d{4}-\\d{2}-\\d{2}";
// `completed = "x" SP date [SP date] [SP description]` (specs/todotxt.abnf line 14).
const COMPLETED_LINE_RE = new RegExp(`^x (${DATE_SRC})(?: (${DATE_SRC}))?(?: (.*))?$`);
// A bare leading date, once any priority has been stripped: the `date [SP description]` arm of
// `incomplete` (specs/todotxt.abnf line 13), read here as the completion's creation date.
const LEADING_DATE_RE = new RegExp(`^(${DATE_SRC})(?: (.*))?$`);
const PRI_TAG_RE = /^pri:([A-Z])$/;

/** Strips a leading `(X) ` or bare `(X)` priority (specs/todotxt.abnf's `priority` rule). */
function stripPriority(raw: string): { letter: string; rest: string } | null {
	const m = raw.match(/^\(([A-Z])\) ?/);
	if (!m) return null;
	return { letter: m[1], rest: raw.slice(m[0].length) };
}

/**
 * `x` chip: on -> prepend `x <today> ` and, if a priority exists, replace it with a trailing
 * `pri:<P>` tag (spec-conformant, `core-complete-pri`); off -> reverse that. Plain string in,
 * plain string out — the caller (`applyChip`) decides where the caret lands afterwards.
 */
export function toggleComplete(raw: string, today: string): string {
	const completed = raw.match(COMPLETED_LINE_RE);
	if (completed) {
		const [, , creationDate, description = ""] = completed;
		const words = description.length > 0 ? description.split(" ") : [];
		let priority: string | null = null;
		const remaining = words.filter((w) => {
			if (priority === null) {
				const m = w.match(PRI_TAG_RE);
				if (m) {
					priority = m[1];
					return false;
				}
			}
			return true;
		});
		const parts: string[] = [];
		if (priority) parts.push(`(${priority})`);
		if (creationDate) parts.push(creationDate);
		if (remaining.length > 0) parts.push(remaining.join(" "));
		return parts.join(" ");
	}

	const stripped = stripPriority(raw);
	const rest = stripped ? stripped.rest : raw;
	const dated = rest.match(LEADING_DATE_RE);
	const creationDate = dated ? dated[1] : null;
	const description = dated ? (dated[2] ?? "") : rest;

	const parts = ["x", today];
	if (creationDate) parts.push(creationDate);
	if (description) parts.push(description);
	if (stripped) parts.push(`pri:${stripped.letter}`);
	return parts.join(" ");
}

/** Priority chip: replaces the current priority, or removes it when the same letter is tapped
 * again. The edit only ever touches the `(X) ` prefix, so any caret at or after it shifts by
 * exactly the prefix's length delta; a caret inside a removed prefix just clamps to 0. */
function applyPriorityChip(raw: string, caret: number, letter: "A" | "B" | "C"): ChipResult {
	const stripped = stripPriority(raw);
	const rest = stripped ? stripped.rest : raw;
	const oldPrefixLen = raw.length - rest.length;
	const removing = stripped?.letter === letter;
	const text = removing ? rest : rest.length > 0 ? `(${letter}) ${rest}` : `(${letter})`;
	const newPrefixLen = text.length - rest.length;
	const newCaret = Math.max(0, Math.min(text.length, caret + (newPrefixLen - oldPrefixLen)));
	return { text, caret: newCaret };
}

/** Every other chip: insert the raw token at the caret. A leading space is added only when the
 * caret isn't already at a word boundary (line start, or preceded by whitespace) — this is the
 * "must not double-space" invariant from notes.md. */
function insertAtCaret(raw: string, caret: number, token: string): ChipResult {
	const before = raw.slice(0, caret);
	const after = raw.slice(caret);
	const atBoundary = before.length === 0 || /\s$/.test(before);
	const insert = atBoundary ? token : ` ${token}`;
	return { text: before + insert + after, caret: before.length + insert.length };
}

/** Applies one chip tap at `caret` in `raw`, returning the new text and where the caret should
 * land. `today` (`YYYY-MM-DD`, local zone — see `localToday`) is only used by the `x` chip. */
export function applyChip(raw: string, caret: number, chip: Chip, today: string): ChipResult {
	switch (chip) {
		case "A":
		case "B":
		case "C":
			return applyPriorityChip(raw, caret, chip);
		case "x": {
			const text = toggleComplete(raw, today);
			return { text, caret: text.length };
		}
		default:
			return insertAtCaret(raw, caret, chip);
	}
}

/** A save that would write back the exact original line is a no-op: no `Apply`, no op-log entry
 * (notes.md "Edge cases & invariants"). */
export function isNoOpEdit(original: string, next: string): boolean {
	return original === next;
}

/** `YYYY-MM-DD` in the browser's local zone (ADR 0011 — same convention as `MutationDto::Complete`'s
 * `today` field, `apps/desktop/src-tauri/src/dto.rs`). */
export function localToday(now: Date = new Date()): string {
	const y = now.getFullYear();
	const m = String(now.getMonth() + 1).padStart(2, "0");
	const d = String(now.getDate()).padStart(2, "0");
	return `${y}-${m}-${d}`;
}

// Crockford base32 (https://www.crockford.com/base32.html), the alphabet ULIDs use — no I L O U,
// matching `specs/todotxt.abnf`'s `ulid` rule.
const CROCKFORD_BASE32 = "0123456789ABCDEFGHJKMNPQRSTVWXYZ";

/** Decodes a ULID's embedded 48-bit millisecond timestamp (first 10 chars) — see the ULID spec,
 * https://github.com/ulid/spec. `HistoryDto`'s ops carry an `op_id` ULID but no separate
 * timestamp field, so this is how the footer gets "how long ago" without a backend change. */
export function decodeUlidTimestampMs(ulid: string): number {
	let ms = 0;
	for (let i = 0; i < 10; i++) {
		const c = ulid[i]?.toUpperCase() ?? "0";
		const value = Math.max(CROCKFORD_BASE32.indexOf(c), 0);
		ms = ms * 32 + value;
	}
	return ms;
}

/** Small, dependency-free relative-time formatter ("3m ago") for the footer. */
export function formatRelativeTime(ms: number, now: number = Date.now()): string {
	const diffSec = Math.max(0, Math.round((now - ms) / 1000));
	if (diffSec < 60) return "just now";
	const diffMin = Math.round(diffSec / 60);
	if (diffMin < 60) return `${diffMin}m ago`;
	const diffHour = Math.round(diffMin / 60);
	if (diffHour < 24) return `${diffHour}h ago`;
	const diffDay = Math.round(diffHour / 24);
	return `${diffDay}d ago`;
}

/** Devices are ULIDs (26 chars); no device-name lookup is exposed to the frontend yet (that's the
 * separate devices-screen task), so the footer just shows a short, still-recognisable prefix. */
export function shortDevice(device: string): string {
	return device.length > 8 ? device.slice(0, 8) : device;
}
