// specs/client-parity.toml, checked from the desktop side (ADR 0031, task
// tasks/tui-revamp/parity-manifest). The manifest's own rules run now; the check that every
// `desktop.status = "done"` row is bound in `keys.ts` with the same keys waits for the desktop
// revamp to create `keys.ts` (tasks/desktop-ui-revamp) and is skipped until then.
//
// The manifest is read with a small parser for the subset it uses (one `key = value` per line;
// values are basic strings, arrays of strings, or inline tables of those), so the desktop app
// takes no TOML dependency for a test. Anything outside that subset fails the parse loudly.
// Ref: https://toml.io/en/v1.0.0 (basic strings, arrays, inline tables),
// https://vitest.dev/api/#test-skip
import { readFileSync } from "node:fs";
import { describe, expect, it } from "vitest";

type Value = string | string[] | { [key: string]: string | string[] };
type Row = Record<string, Value>;

const MANIFEST = new URL("../../../../specs/client-parity.toml", import.meta.url);
const STATUSES = ["done", "planned", "differs", "na"];
const SCOPES = [
	"global",
	"list",
	"edit",
	"search",
	"prompt",
	"detail",
	"universal",
	"settings",
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
function parseManifest(text: string): { action: Row[]; screen: Row[] } {
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

const manifest = parseManifest(readFileSync(MANIFEST, "utf8"));

describe("specs/client-parity.toml", () => {
	it("has actions and screens", () => {
		expect(manifest.action.length).toBeGreaterThan(0);
		expect(manifest.screen.length).toBeGreaterThan(0);
	});

	it("gives every action a unique dotted id, a scope and both clients' status", () => {
		const ids = manifest.action.map((a) => a.id);
		expect(new Set(ids).size).toBe(ids.length);
		for (const a of manifest.action) {
			expect(a.id, JSON.stringify(a)).toMatch(/^[a-z_]+\.[a-z_]+$/);
			expect(SCOPES, `${a.id} scope`).toContain(a.scope);
			expect(Array.isArray(a.keys), `${a.id} keys`).toBe(true);
			for (const client of ["desktop", "tui"] as const) {
				const side = a[client] as { status?: string };
				expect(STATUSES, `${a.id} ${client}`).toContain(side.status);
			}
		}
	});

	it("never leaves a differs or na row without its deviation", () => {
		for (const a of manifest.action) {
			const statuses = ["desktop", "tui"].map((c) => (a[c] as { status: string }).status);
			if (statuses.some((s) => s === "differs" || s === "na")) {
				expect(String(a.deviation), a.id as string).not.toBe("");
			}
		}
	});

	// keys.ts does not exist yet (tasks/desktop-ui-revamp): once it exports its bindings, compare
	// every `desktop.status = "done"` row's keys (its own `desktop.keys` when present) with them,
	// and every binding back to a row.
	it.skip("matches keys.ts: done rows are bound with the same keys, and nothing else is", () => {});
});
