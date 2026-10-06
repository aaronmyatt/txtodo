// The reference table of keys (task desktop-ui-revamp) that Settings > Shortcuts and the Help page
// render: the shell's own bindings straight from `keys.ts`, plus the keys that live inside the
// editors, the Universal list and the prompt bar (their own keymaps), and the global hotkey the
// Rust side registers. One list, so the two pages never disagree.
import { BINDINGS } from "./keys";

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

const shell = (id: string) => BINDINGS.find((b) => b.id === id);

function fromBindings(ids: string[]): KeyRow[] {
	return ids.flatMap((id) => {
		const b = shell(id);
		return b ? [{ keys: b.keys.map(presses), title: b.title }] : [];
	});
}

/** Every group, with `hotkey` (from `quick_add_hotkey`, e.g. `["Super", "Shift", "Space"]`)
 * heading the list; an empty `hotkey` leaves that row out. */
export function keyGroups(hotkey: string[]): KeyGroup[] {
	const anywhere: KeyRow[] = hotkey.length > 0 ? [{ keys: [[hotkey]], title: "Quick Add from any app" }] : [];
	return [
		...(anywhere.length > 0 ? [{ title: "Anywhere", rows: anywhere }] : []),
		{
			title: "Buffer",
			rows: [
				{ keys: [presses("Mod-Enter")], title: "Open the detail for the line" },
				{ keys: [presses("Mod-s")], title: "Save now (it also saves on blur)" },
				{ keys: [presses("Escape")], title: "Discard the edit" },
				// The main list has no undo history of its own (FileView has no history()); the
				// one-line editors and notes do.
				{ keys: [presses("Mod-z")], title: "Undo in the prompt bar and detail (not the list)" }
			]
		},
		{
			title: "Search",
			rows: [
				...fromBindings(["search.focus"]),
				{ keys: [presses("Enter"), presses("Shift-Enter")], title: "Next / previous match" }
			]
		},
		{ title: "Go to", rows: fromBindings(["nav.tasks", "nav.universal", "nav.settings", "nav.help"]) },
		{
			title: "Universal",
			rows: [
				{ keys: [presses("j"), presses("k")], title: "Move" },
				{ keys: [presses("Enter")], title: "Open its detail below" },
				{ keys: [presses("x")], title: "Complete the focused task" }
			]
		},
		{
			title: "Prompt bar",
			rows: [
				{ keys: [presses("Enter")], title: "Add the task" },
				{ keys: [presses("Escape")], title: "Back to the buffer" }
			]
		}
	];
}

/** How a key reads on screen: macOS symbols on a Mac, words elsewhere. */
export function keyLabel(key: string, mac: boolean): string {
	const MAC: Record<string, string> = { Mod: "⌘", Super: "⌘", Shift: "⇧", Alt: "⌥", Ctrl: "⌃", Enter: "↵", Escape: "Esc" };
	const OTHER: Record<string, string> = { Mod: "Ctrl", Super: "Win", Enter: "Enter", Escape: "Esc" };
	return (mac ? MAC : OTHER)[key] ?? key;
}
