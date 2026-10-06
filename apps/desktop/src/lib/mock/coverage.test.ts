// Every Tauri command the app invokes has an answer in the dev mock (task desktop-ui-revamp, line
// "Extend the Tauri shim"), so `npm run dev` without a daemon never hits "unhandled command".
// Reads the sources as text: https://nodejs.org/api/fs.html#fsreaddirsyncpath-options
import { readdirSync, readFileSync, statSync } from "node:fs";
import { join } from "node:path";
import { describe, expect, it } from "vitest";

const SRC = join(__dirname, "..", "..");

function files(dir: string): string[] {
	return readdirSync(dir).flatMap((name) => {
		const path = join(dir, name);
		if (statSync(path).isDirectory()) return name === "wasm-core" ? [] : files(path);
		return /\.(ts|svelte)$/.test(name) && !name.endsWith(".test.ts") ? [path] : [];
	});
}

describe("dev mock coverage", () => {
	it("answers every invoked command", () => {
		const invoked = new Set<string>();
		for (const f of files(SRC)) {
			for (const m of readFileSync(f, "utf8").matchAll(/invoke(?:<[^>]*>)?\("([a-z_]+)"/g)) invoked.add(m[1]);
		}
		const mock = readFileSync(join(SRC, "lib", "mock", "tauriMock.ts"), "utf8");
		const handled = new Set([...mock.matchAll(/case "([a-z_]+)"/g)].map((m) => m[1]));
		// ui_log goes straight to Tauri (tauriShim.ts never routes it through the mock).
		const missing = [...invoked].filter((c) => !handled.has(c) && c !== "ui_log").sort();
		expect(missing).toEqual([]);
	});
});
