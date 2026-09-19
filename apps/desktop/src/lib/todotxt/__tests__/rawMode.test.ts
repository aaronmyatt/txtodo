// Unit tests for raw mode's pure logic ($lib/todotxt/rawMode.ts). No CodeMirror, no Tauri, no
// Svelte runtime — see that module's own doc comment for why the delta is line-level, not a
// whole-string replace. Component/DOM-level behaviors (Esc discards, blur/Cmd-S commits, file
// switch commits-or-discards, badge aria-pressed) are covered by e2e specs instead, matching this
// project's existing split (vitest.config.ts: "pure-logic unit tests only, no Svelte component
// runtime").
import { describe, expect, it } from "vitest";
import { canEnterRawMode, computeDelta, isNoOpSave } from "../rawMode";

const LINE_A = "(A) buy milk id:01ARZ3NDEKTSV4RRFFQ69G5FAV";
const LINE_B = "call mum id:01ARZ3NDEKTSV4RRFFQ69G5FA2";

describe("isNoOpSave", () => {
	it("is true for an identical buffer", () => {
		expect(isNoOpSave(`${LINE_A}\n${LINE_B}\n`, `${LINE_A}\n${LINE_B}\n`)).toBe(true);
	});

	it("is false for any change", () => {
		expect(isNoOpSave(`${LINE_A}\n`, `${LINE_A} +home\n`)).toBe(false);
	});
});

describe("computeDelta — identical save", () => {
	it("returns no mutations for an unchanged buffer", () => {
		const text = `${LINE_A}\n${LINE_B}\n`;
		expect(computeDelta(text, text)).toEqual([]);
	});

	it("returns no mutations for two empty buffers", () => {
		expect(computeDelta("", "")).toEqual([]);
	});
});

describe("computeDelta — edits (line-diff, not whole-string)", () => {
	it("emits exactly one Edit for a one-line change, keyed by the line's id: tag", () => {
		const baseline = `${LINE_A}\n${LINE_B}\n`;
		const next = `(A) buy oat milk id:01ARZ3NDEKTSV4RRFFQ69G5FAV\n${LINE_B}\n`;
		const mutations = computeDelta(baseline, next);
		expect(mutations).toEqual([
			{
				kind: "edit",
				task: { line_number: 1, task_id: "01ARZ3NDEKTSV4RRFFQ69G5FAV" },
				new_line: "(A) buy oat milk id:01ARZ3NDEKTSV4RRFFQ69G5FAV"
			}
		]);
	});

	it("never emits a whole-buffer replace: unrelated untouched lines produce no mutation", () => {
		const baseline = `${LINE_A}\n${LINE_B}\nthird line\n`;
		const next = `${LINE_A} +home\n${LINE_B}\nthird line\n`;
		const mutations = computeDelta(baseline, next);
		expect(mutations).toHaveLength(1);
		expect(mutations[0]).toMatchObject({ kind: "edit" });
	});

	it("edits both lines that changed, in document order", () => {
		const baseline = `${LINE_A}\n${LINE_B}\n`;
		const next = `${LINE_A} +home\ncall dad id:01ARZ3NDEKTSV4RRFFQ69G5FA2\n`;
		const mutations = computeDelta(baseline, next);
		expect(mutations).toEqual([
			{
				kind: "edit",
				task: { line_number: 1, task_id: "01ARZ3NDEKTSV4RRFFQ69G5FAV" },
				new_line: `${LINE_A} +home`
			},
			{
				kind: "edit",
				task: { line_number: 2, task_id: "01ARZ3NDEKTSV4RRFFQ69G5FA2" },
				new_line: "call dad id:01ARZ3NDEKTSV4RRFFQ69G5FA2"
			}
		]);
	});
});

describe("computeDelta — adds", () => {
	it("emits Add for a brand-new untagged line", () => {
		const baseline = `${LINE_A}\n`;
		const next = `${LINE_A}\nwater the plants\n`;
		expect(computeDelta(baseline, next)).toEqual([{ kind: "add", line: "water the plants" }]);
	});

	// tasks/desktop-concurrent-edit-loss root cause 1: a trailing `\n` is a terminator, not its own
	// blank line — `next` here (typed on the "Add a line…" row, no trailing newline yet) must not
	// produce a phantom `delete` alongside the `add`, or the delete lands on the line just added.
	it("emits only Add when typing a new last line, no phantom trailing-newline delete", () => {
		expect(computeDelta("a\nb\n", "a\nb\nnew")).toEqual([{ kind: "add", line: "new" }]);
	});
});

describe("computeDelta — deletes", () => {
	it("emits Delete for a removed id-tagged line", () => {
		const baseline = `${LINE_A}\n${LINE_B}\n`;
		const next = `${LINE_B}\n`;
		expect(computeDelta(baseline, next)).toEqual([
			{
				kind: "delete",
				task: { line_number: 1, task_id: "01ARZ3NDEKTSV4RRFFQ69G5FAV" },
				leave_blank: false
			}
		]);
	});

	it("emits Delete (task_id: \"\") for a removed untagged/blank line", () => {
		const baseline = `${LINE_A}\n\nsome scratch note\n`;
		const next = `${LINE_A}\n\n`;
		expect(computeDelta(baseline, next)).toEqual([
			{ kind: "delete", task: { line_number: 3, task_id: "" }, leave_blank: false }
		]);
	});

	it("does not delete an untagged line that only moved", () => {
		const baseline = `scratch note\n${LINE_A}\n`;
		const next = `${LINE_A}\nscratch note\n`;
		expect(computeDelta(baseline, next)).toEqual([]);
	});
});

describe("computeDelta — mixed", () => {
	it("combines an edit, an add, and a delete from one buffer rewrite", () => {
		const baseline = `${LINE_A}\n${LINE_B}\n`;
		const next = `(A) buy oat milk id:01ARZ3NDEKTSV4RRFFQ69G5FAV\nnew task\n`;
		const mutations = computeDelta(baseline, next);
		expect(mutations).toEqual([
			{
				kind: "edit",
				task: { line_number: 1, task_id: "01ARZ3NDEKTSV4RRFFQ69G5FAV" },
				new_line: "(A) buy oat milk id:01ARZ3NDEKTSV4RRFFQ69G5FAV"
			},
			{ kind: "add", line: "new task" },
			{
				kind: "delete",
				task: { line_number: 2, task_id: "01ARZ3NDEKTSV4RRFFQ69G5FA2" },
				leave_blank: false
			}
		]);
	});
});

describe("canEnterRawMode", () => {
	it("refuses when a needs_review flag is already showing", () => {
		expect(canEnterRawMode(true)).toBe(false);
	});

	it("allows when there is nothing pending review", () => {
		expect(canEnterRawMode(false)).toBe(true);
	});
});
