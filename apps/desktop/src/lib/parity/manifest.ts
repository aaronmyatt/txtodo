// specs/client-parity.toml (ADR 0031) for the app itself: the Help page and Settings > Shortcuts
// render their key tables from it, and keys.parity.test.ts checks keys.ts against it. The file is
// bundled as text at build time (Vite's `?raw`: https://vite.dev/guide/assets#importing-asset-as-string)
// and read with a small parser for the subset it uses (one `key = value` per line; values are
// basic strings, arrays of strings, or inline tables of those), so the app takes no TOML
// dependency. Anything outside that subset fails the parse loudly.
// Ref: https://toml.io/en/v1.0.0 (basic strings, arrays, inline tables)
import manifestText from "../../../../../specs/client-parity.toml?raw";

export type Value = string | string[] | { [key: string]: string | string[] };
export type Row = Record<string, Value>;

export const STATUSES = ["done", "planned", "differs", "na"];
export const SCOPES = [
	"global",
	"list",
	"edit",
	"search",
	"prompt",
	"detail",
	"universal",
	"settings",
	"help",
	"sheet"
];

/** A basic string at the start of `text`: its value and the rest of `text`. */
function takeString(text: string): [string, string] {
	const m = /^"((?:[^"\\]|\\.)*)"/.exec(text);
	if (!m) throw new Error(`expected a string at: ${text}`);
	return [m[1].replace(/\\(.)/g, "$1"), text.slice(m[0].length).trimStart()];
}

/** A string, an array of strings, or an inline table of those, at the start of `text`. */
function takeValue(text: string): [Value, string] {
	if (text.startsWith('"')) return takeString(text);
	if (text.startsWith("[")) {
		const out: string[] = [];
		let rest = text.slice(1).trimStart();
		while (!rest.startsWith("]")) {
			const [s, after] = takeString(rest);
			out.push(s);
			rest = after.startsWith(",") ? after.slice(1).trimStart() : after;
		}
		return [out, rest.slice(1).trimStart()];
	}
	if (text.startsWith("{")) {
		const table: { [key: string]: string | string[] } = {};
		let rest = text.slice(1).trimStart();
		while (!rest.startsWith("}")) {
			const m = /^([A-Za-z_]+)\s*=\s*/.exec(rest);
			if (!m) throw new Error(`expected a key at: ${rest}`);
			const [v, after] = takeValue(rest.slice(m[0].length));
			if (typeof v !== "string" && !Array.isArray(v)) throw new Error("nested table");
			table[m[1]] = v;
			rest = after.startsWith(",") ? after.slice(1).trimStart() : after;
		}
		return [table, rest.slice(1).trimStart()];
	}
	throw new Error(`unsupported value: ${text}`);
}

/** Every `[[action]]` and `[[screen]]` row, in file order. */
export function parseManifest(text: string): { action: Row[]; screen: Row[] } {
	const out: { action: Row[]; screen: Row[] } = { action: [], screen: [] };
	let row: Row | null = null;
	for (const raw of text.split("\n")) {
		const line = raw.trim();
		if (line === "" || line.startsWith("#")) continue;
		const header = /^\[\[(action|screen)\]\]$/.exec(line);
		if (header) {
			row = {};
			out[header[1] as "action" | "screen"].push(row);
			continue;
		}
		const m = /^([A-Za-z_]+)\s*=\s*(.*)$/.exec(line);
		if (!m || !row) throw new Error(`unexpected line: ${raw}`);
		const [value, rest] = takeValue(m[2]);
		if (rest !== "") throw new Error(`trailing text: ${raw}`);
		row[m[1]] = value;
	}
	return out;
}

/** One client's side of a row. */
export interface Side {
	status: string;
	surface?: string;
	keys?: string[];
}

/** An `[[action]]` row, typed. */
export interface Action {
	id: string;
	title: string;
	keys: string[];
	scope: string;
	desktop: Side;
	tui: Side;
	deviation: string;
}

export function actionsOf(text: string): Action[] {
	return parseManifest(text).action.map((r) => ({
		id: String(r.id),
		title: String(r.title),
		keys: (r.keys as string[]) ?? [],
		scope: String(r.scope),
		desktop: r.desktop as unknown as Side,
		tui: r.tui as unknown as Side,
		deviation: String(r.deviation ?? "")
	}));
}

/** The manifest's actions, from the bundled file. */
export const ACTIONS: Action[] = actionsOf(manifestText);

/** The keys the desktop binds for `a`: its own `desktop.keys` when given, else the row's. */
export function desktopKeys(a: Action): string[] {
	return a.desktop.keys ?? a.keys;
}
