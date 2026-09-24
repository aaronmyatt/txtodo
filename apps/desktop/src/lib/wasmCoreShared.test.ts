// The shared-logic exports of the regenerated wasm core (crates/txtodo-ffi/src/wasm.rs, task
// tasks/tui-revamp/shared-core) load and answer the way txtodo-core's own tests say, so desktop
// can switch its TS copies over (the @parity lines in tasks/desktop-ui-revamp/todo.txt).
// Loaded with `initSync` from the committed .wasm file: vitest runs in Node, where the default
// `init()` would try to fetch it.
// Ref: https://rustwasm.github.io/docs/wasm-bindgen/reference/deployment.html#without-a-bundler
import { readFileSync } from "node:fs";
import { beforeAll, describe, expect, it } from "vitest";
import {
	apply_chip,
	due_bucket,
	due_label,
	group_rows,
	initSync,
	matches_query,
	strict_hint,
	toggle_complete_text
} from "./wasm-core/txtodo_ffi";

const TODAY = "2026-09-25";

beforeAll(() => {
	const bytes = readFileSync(new URL("./wasm-core/txtodo_ffi_bg.wasm", import.meta.url));
	initSync({ module: bytes });
});

describe("wasm core: shared logic", () => {
	it("matches_query ANDs terms, excludes -terms and knows is:done", () => {
		expect(matches_query("(A) Call Mum @phone", "mum @PHONE")).toBe(true);
		expect(matches_query("(A) Call Mum @phone", "-phone")).toBe(false);
		expect(matches_query("x 2026-09-25 file taxes", "is:done")).toBe(true);
	});

	it("strict_hint names the first slip or returns nothing", () => {
		expect(strict_hint("(a) nope")).toBe("Priority letters are uppercase: (A)–(Z).");
		expect(strict_hint("(A) ok due:2026-09-26") ?? null).toBeNull();
	});

	it("apply_chip keeps carets in UTF-16 units", () => {
		expect(apply_chip("call mum", 0, "A", TODAY)).toEqual({ text: "(A) call mum", caret: 4 });
		expect(apply_chip("call 😀", 7, "+", TODAY)).toEqual({ text: "call 😀 +", caret: 9 });
		expect(apply_chip("x", 0, "nope", TODAY)).toBeNull();
	});

	it("toggle_complete_text round-trips a priority through pri:", () => {
		const done = toggle_complete_text("(A) 2026-09-01 call mum", TODAY);
		expect(done).toBe("x 2026-09-25 2026-09-01 call mum pri:A");
		expect(toggle_complete_text(done, TODAY)).toBe("(A) 2026-09-01 call mum");
	});

	it("due_bucket and due_label follow the mockup", () => {
		expect(due_bucket("2026-09-23", TODAY)).toBe("Overdue");
		expect(due_bucket(undefined, TODAY)).toBe("No date");
		expect(due_label("2026-09-26", TODAY)).toEqual({ text: "tomorrow", days: 1 });
		expect(due_label("soon", TODAY)).toBeNull();
	});

	it("group_rows groups by priority with the unprioritised last", () => {
		const rows = [
			{ done: false, workspace: "home" },
			{ done: false, priority: "B", workspace: "work" }
		];
		expect(group_rows(rows, "priority", TODAY, ["home", "work"])).toEqual([
			{ name: "(B)", rows: [1] },
			{ name: "No priority", rows: [0] }
		]);
		expect(group_rows(rows, "size", TODAY, [])).toBeNull();
	});
});
