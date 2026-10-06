// Pure logic for `EditPopover.svelte` — no DOM, no Tauri, no CodeMirror, so it is plain-function
// testable (tasks/desktop-edit-popover/notes.md, plan §3.2). Grammar reference:
// `specs/todotxt.abnf` (`priority`, `completed`/`incomplete`, `pri-tag`). What a chip does to the
// text lives in the shared core (`txtodo_core::chips`, via `wasmCore.applyChipCore`), not here.

/** Token chips per plan §3.2 / notes.md's `Chip` union — the exact nine the popover renders. */
export type Chip = "A" | "B" | "C" | "+" | "@" | "due:" | "t:" | "rec:" | "x";

/** The nine chips in plan §3.2's order: priority, completion, then the insert-at-caret tokens. */
export const CHIPS: { chip: Chip; label: string }[] = [
	{ chip: "A", label: "(A)" },
	{ chip: "B", label: "(B)" },
	{ chip: "C", label: "(C)" },
	{ chip: "x", label: "x" },
	{ chip: "+", label: "+" },
	{ chip: "@", label: "@" },
	{ chip: "due:", label: "due:" },
	{ chip: "t:", label: "t:" },
	{ chip: "rec:", label: "rec:" }
];

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
