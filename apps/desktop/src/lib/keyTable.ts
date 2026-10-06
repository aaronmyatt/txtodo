// The keys reference that Settings > Shortcuts and the Help page render (task desktop-ui-revamp),
// built from specs/client-parity.toml (`./parity/manifest.ts`): every row the desktop has built
// with keys, grouped by scope, in the manifest's order and words. A "done" row uses the row's keys;
// a "differs" row only shows when it names its own `desktop.keys` (otherwise the keys listed are
// another client's).
// The manifest is the one list every client and both pages read, so they never disagree; keys.ts
// is checked against it (keys.parity.test.ts).
import { ACTIONS, desktopKeys, type Action } from "./parity/manifest";

export interface KeyRow {
	/** Each alternative is a sequence of presses; each press is keys held together. */
	keys: string[][][];
	title: string;
}

export interface KeyGroup {
	title: string;
	rows: KeyRow[];
}

/** `"g t"` (a chord) as presses; `"Mod-Enter"` as one press of Mod + Enter. */
function presses(binding: string): string[][] {
	return binding.split(" ").map((p) => p.split(/-(?=.)/));
}

/** Group titles by manifest scope, in the order the pages show them. */
const SCOPE_TITLES: [string, string][] = [
	["global", "Anywhere"],
	["list", "Tasks list (Esc first, so nothing has the keyboard)"],
	["edit", "Editing a line"],
	["search", "Search"],
	["prompt", "Prompt bar"],
	["detail", "Detail"],
	["universal", "Universal"],
	["sheet", "Conflict review"],
	["settings", "Settings"],
	["help", "Help"]
];

/** Every group with at least one row, from `actions` (the bundled manifest by default). */
export function keyGroups(actions: Action[] = ACTIONS): KeyGroup[] {
	const shown = (a: Action) =>
		a.desktop.status === "done" || (a.desktop.status === "differs" && a.desktop.keys !== undefined);
	const built = actions.filter((a) => shown(a) && desktopKeys(a).length > 0);
	return SCOPE_TITLES.map(([scope, title]) => ({
		title,
		rows: built.filter((a) => a.scope === scope).map((a) => ({ keys: desktopKeys(a).map(presses), title: a.title }))
	})).filter((g) => g.rows.length > 0);
}

/** How a key reads on screen: macOS symbols on a Mac, words elsewhere. */
export function keyLabel(key: string, mac: boolean): string {
	const BOTH: Record<string, string> = { Down: "↓", Up: "↑", Left: "←", Right: "→", Enter: "↵" };
	const MAC: Record<string, string> = { Mod: "⌘", Super: "⌘", Shift: "⇧", Alt: "⌥", Ctrl: "⌃", Escape: "Esc" };
	const OTHER: Record<string, string> = { Mod: "Ctrl", Super: "Win", Escape: "Esc", Enter: "Enter" };
	return (mac ? MAC : OTHER)[key] ?? BOTH[key] ?? key;
}
