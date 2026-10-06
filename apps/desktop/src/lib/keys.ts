// The desktop app's own keys outside the editors (task desktop-ui-revamp): one table, named and
// scoped as specs/client-parity.toml names them, plus the chord resolver and the "is the reader
// typing" guard. AppShell feeds window keydowns through here; the Help page and Settings >
// Shortcuts render from the same table (their own lines). Keys inside CodeMirror (Mod-s, Esc,
// Mod-Enter) stay in the editors' own keymaps.
// Ref: https://developer.mozilla.org/en-US/docs/Web/API/KeyboardEvent/key

/** One binding: the manifest id (also what the shell runs), its keys, and a short title.
 * `list` bindings act on the Tasks buffer's current line, only while nothing has the keyboard
 * (list mode: after Esc, or before anything is clicked). */
export interface Binding {
	id: string;
	keys: readonly string[];
	title: string;
	scope?: "list";
}

/** The shell's bindings, in the manifest's names (`g t` is a chord: g, then t within 900 ms). */
export const BINDINGS: readonly Binding[] = [
	{ id: "search.focus", keys: ["/"], title: "Focus the search" },
	{ id: "nav.tasks", keys: ["g t"], title: "Go to Tasks" },
	{ id: "nav.universal", keys: ["g u"], title: "Go to Universal" },
	{ id: "nav.settings", keys: ["g s"], title: "Go to Settings" },
	{ id: "nav.help", keys: ["?"], title: "Open Help" },
	// List mode (specs/client-parity.toml list.*, the TUI's keys).
	{ id: "list.down", keys: ["j", "Down"], title: "Next line", scope: "list" },
	{ id: "list.up", keys: ["k", "Up"], title: "Previous line", scope: "list" },
	{ id: "list.first", keys: ["g g"], title: "First line", scope: "list" },
	{ id: "list.last", keys: ["G"], title: "Last line", scope: "list" },
	{ id: "list.edit_start", keys: ["i"], title: "Edit the line, caret at the start", scope: "list" },
	{ id: "list.edit_end", keys: ["a", "A"], title: "Edit the line, caret at the end", scope: "list" },
	{ id: "list.toggle_complete", keys: ["x", "Space"], title: "Complete or reopen the line", scope: "list" },
	{ id: "list.delete", keys: ["d d"], title: "Delete the line", scope: "list" },
	{ id: "list.move_down", keys: ["J", "Alt-Down"], title: "Move the line down", scope: "list" },
	{ id: "list.move_up", keys: ["K", "Alt-Up"], title: "Move the line up", scope: "list" },
	{ id: "list.undo", keys: ["u"], title: "Undo this session's last change", scope: "list" },
	{ id: "detail.open", keys: ["Enter"], title: "Open the line's detail", scope: "list" }
];

/** Whether binding `id` is a list-mode one. */
export function isListCommand(id: string): boolean {
	return BINDINGS.some((b) => b.id === id && b.scope === "list");
}

/** How long the second key of a chord may wait (the manifest's 900 ms). */
export const CHORD_MS = 900;

/** The parts of a keydown the names need (a real `KeyboardEvent` fits). */
export interface KeyLike {
	key: string;
	ctrlKey: boolean;
	metaKey: boolean;
	altKey: boolean;
}

/** The manifest's name for a key press, or null for a bare modifier. A printable key is itself
 * (`?`, `G`), modified keys are `Ctrl-`/`Alt-`/`Super-` joined. */
export function keyName(e: KeyLike): string | null {
	if (["Shift", "Control", "Alt", "Meta"].includes(e.key)) return null;
	// The manifest's names for the keys whose DOM names differ (`ArrowDown` is `Down`).
	const NAMES: Record<string, string> = {
		" ": "Space",
		ArrowDown: "Down",
		ArrowUp: "Up",
		ArrowLeft: "Left",
		ArrowRight: "Right",
		Escape: "Esc"
	};
	const key = NAMES[e.key] ?? e.key;
	const mods = `${e.metaKey ? "Super-" : ""}${e.ctrlKey ? "Ctrl-" : ""}${e.altKey ? "Alt-" : ""}`;
	return mods + key;
}

/** What a key resolved to. */
export type Resolved = { kind: "command"; id: string } | { kind: "pending" } | { kind: "none" };

/** Chord state: the first key of a chord and when it came. */
export class Chords {
	private pending: { key: string; at: number } | null = null;

	constructor(private readonly bindings: readonly Binding[] = BINDINGS) {}

	/** Resolves `name` pressed at `now` (ms): a command, the first half of a chord, or nothing. A
	 * key that does not complete a pending chord drops it and is resolved on its own. */
	resolve(name: string, now: number): Resolved {
		const first = this.pending;
		this.pending = null;
		if (first && now - first.at <= CHORD_MS) {
			const hit = this.find(`${first.key} ${name}`);
			if (hit) return { kind: "command", id: hit.id };
		}
		const single = this.find(name);
		if (single) return { kind: "command", id: single.id };
		if (this.bindings.some((b) => b.keys.some((k) => k.startsWith(`${name} `)))) {
			this.pending = { key: name, at: now };
			return { kind: "pending" };
		}
		return { kind: "none" };
	}

	private find(keys: string): Binding | undefined {
		return this.bindings.find((b) => b.keys.includes(keys));
	}
}

/** Whether a keydown on `target` belongs to whatever the reader is typing in: a form field, an
 * editable element (CodeMirror's content is contenteditable), or anything inside a dialog. */
export function isTypingTarget(target: EventTarget | null): boolean {
	if (!(target instanceof Element)) return false;
	if (target.closest("input, textarea, select, [contenteditable=''], [contenteditable='true']")) return true;
	if (target.closest("[role='dialog'], dialog")) return true;
	return target instanceof HTMLElement && target.isContentEditable;
}
