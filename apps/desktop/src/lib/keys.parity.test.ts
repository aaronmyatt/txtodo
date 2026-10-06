// specs/client-parity.toml, checked from the desktop side (ADR 0031, task
// tasks/tui-revamp/parity-manifest): the manifest's own rules, and keys.ts against it both ways.
// The parser is the app's own (`./parity/manifest.ts`), which Help and Shortcuts render from.
// Ref: https://vitest.dev/api/
import { readFileSync } from "node:fs";
import { describe, expect, it } from "vitest";
import { BINDINGS } from "./keys";
import { ACTIONS, desktopKeys, parseManifest, SCOPES, STATUSES } from "./parity/manifest";

const manifest = parseManifest(readFileSync(new URL("../../../../specs/client-parity.toml", import.meta.url), "utf8"));

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

	// The shell's own keys (keys.ts) against the manifest: each binding names a built row and
	// binds only keys that row lists for the desktop; each built row keys.ts owns (nav.*,
	// search.focus, the list-mode rows) is bound. Keys another surface binds (CodeMirror's
	// Mod-Enter for detail.open) may sit beside a binding's.
	it("matches keys.ts: bindings are built rows with their keys, and keys.ts rows are bound", () => {
		const built = (s: string) => s === "done" || s === "differs";
		for (const b of BINDINGS) {
			const row = ACTIONS.find((a) => a.id === b.id);
			expect(row, `${b.id} has a manifest row`).toBeDefined();
			expect(built(row!.desktop.status), `${b.id} is built for the desktop`).toBe(true);
			for (const k of b.keys) expect(desktopKeys(row!), `${b.id} lists ${k}`).toContain(k);
		}
		const owned = (id: string) =>
			id.startsWith("nav.") || id === "search.focus" || ["list", "sync", "offers"].includes(id.split(".")[0]) || id === "detail.open";
		for (const a of ACTIONS) {
			if (!owned(a.id) || !built(a.desktop.status) || desktopKeys(a).length === 0) continue;
			if (a.scope !== "list" && a.scope !== "global") continue;
			expect(BINDINGS.map((b) => b.id), `${a.id} is bound in keys.ts`).toContain(a.id);
		}
	});
});
