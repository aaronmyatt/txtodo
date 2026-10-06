// Editor preferences (task desktop-ui-revamp, Settings > Appearance): text size, line height, line
// numbers and the 100-character hint, persisted to localStorage like the theme (the quick-add
// webview shares the origin). `+layout.svelte` puts the sizes on <html> as --editor-font-size and
// --editor-line-height, which every FileView and the detail parent line read; FileView switches
// its gutter and long-line hint from the booleans.
// Ref: https://developer.mozilla.org/en-US/docs/Web/API/Window/localStorage
import { writable } from "svelte/store";

// Not `$app/environment`: this file stays importable from plain-Node vitest, like pin.ts.
const browser = typeof window !== "undefined" && typeof localStorage !== "undefined";

export interface EditorPrefs {
	/** Text size in px. */
	fontSize: number;
	/** Line height in px (px, not a ratio: decoration row maths assume it). */
	lineHeight: number;
	lineNumbers: boolean;
	/** Underline lines past 100 characters. */
	lengthHint: boolean;
}

export const DEFAULT_PREFS: EditorPrefs = { fontSize: 15, lineHeight: 28, lineNumbers: true, lengthHint: true };

/** The allowed ranges; a stored value outside them falls back to the default. */
export const FONT_SIZES = { min: 11, max: 24 };
export const LINE_HEIGHTS = { min: 16, max: 44 };

const STORAGE_KEY = "txtodo-editor-prefs";

/** Prefs from stored JSON, each field checked on its own; anything wrong is the default. */
export function parsePrefs(raw: string | null): EditorPrefs {
	let value: Partial<Record<keyof EditorPrefs, unknown>> = {};
	try {
		const parsed = raw ? JSON.parse(raw) : {};
		if (parsed && typeof parsed === "object") value = parsed;
	} catch {
		// not JSON: defaults
	}
	const within = (n: unknown, r: { min: number; max: number }, d: number) =>
		typeof n === "number" && Number.isInteger(n) && n >= r.min && n <= r.max ? n : d;
	const flag = (b: unknown, d: boolean) => (typeof b === "boolean" ? b : d);
	return {
		fontSize: within(value.fontSize, FONT_SIZES, DEFAULT_PREFS.fontSize),
		lineHeight: within(value.lineHeight, LINE_HEIGHTS, DEFAULT_PREFS.lineHeight),
		lineNumbers: flag(value.lineNumbers, DEFAULT_PREFS.lineNumbers),
		lengthHint: flag(value.lengthHint, DEFAULT_PREFS.lengthHint)
	};
}

function readStored(): EditorPrefs {
	if (!browser) return DEFAULT_PREFS;
	try {
		return parsePrefs(localStorage.getItem(STORAGE_KEY));
	} catch {
		return DEFAULT_PREFS;
	}
}

export const editorPrefs = writable<EditorPrefs>(readStored());

/** Changes some prefs and stores the result. */
export function setEditorPrefs(change: Partial<EditorPrefs>): void {
	editorPrefs.update((current) => {
		const next = parsePrefs(JSON.stringify({ ...current, ...change }));
		if (browser) {
			try {
				localStorage.setItem(STORAGE_KEY, JSON.stringify(next));
			} catch {
				// storage refused (private mode, quota): the change still applies for this session
			}
		}
		return next;
	});
}
