// Unit tests for the pending-conflicts store (tasks/desktop-conflict-review/todo.txt items 10-12
// cover the store-update/resolve/dismiss guarantees; the daemon-injection half of those items is
// an integration concern for a harness that runs a second daemon, out of scope here).
//
// NOTE: apps/desktop/package.json has no test runner wired up yet (no vitest/jest devDependency,
// no "test" script) — see this task's report. This file follows Vitest's conventions (the de
// facto default for a Vite/SvelteKit project) so it is ready to run as soon as `vitest` is added
// as a devDependency and a "test" script calls it; until then it is not executed by `npm run
// check` or `npm run build`. Once wired up: `npx vitest run`.
// Ref: https://vitest.dev/guide/
import { describe, expect, it } from "vitest";
import { get } from "svelte/store";
import {
	flagsForPath,
	isResurrectCandidate,
	pendingConflictCount,
	pendingConflicts
} from "./conflicts";
import type { ReviewFlag } from "../daemon";

function flag(overrides: Partial<ReviewFlag> = {}): ReviewFlag {
	return {
		task_id: "01T1",
		line_number: 3,
		mine: "buy milk",
		theirs: "buy milkk",
		...overrides
	};
}

describe("pendingConflicts store", () => {
	it("starts empty", () => {
		pendingConflicts.clear();
		expect(get(pendingConflictCount)).toBe(0);
	});

	it("addFlags adds a new flag for a path, incrementing the count", () => {
		pendingConflicts.clear();
		pendingConflicts.addFlags("todo.txt", [flag()]);
		expect(get(pendingConflictCount)).toBe(1);
		expect(flagsForPath(get(pendingConflicts), "todo.txt")).toHaveLength(1);
	});

	it("addFlags is additive across separate calls (simulating multiple Watch events)", () => {
		pendingConflicts.clear();
		pendingConflicts.addFlags("todo.txt", [flag({ task_id: "a" })]);
		pendingConflicts.addFlags("todo.txt", [flag({ task_id: "b" })]);
		expect(get(pendingConflictCount)).toBe(2);
	});

	it("addFlags keeps flags scoped to their own path", () => {
		pendingConflicts.clear();
		pendingConflicts.addFlags("todo.txt", [flag({ task_id: "a" })]);
		pendingConflicts.addFlags("done.txt", [flag({ task_id: "b" })]);
		expect(flagsForPath(get(pendingConflicts), "todo.txt").map((f) => f.task_id)).toEqual(["a"]);
		expect(get(pendingConflictCount)).toBe(2);
	});

	it("remove drops exactly the resolved flag (simulating a successful resolve)", () => {
		pendingConflicts.clear();
		pendingConflicts.addFlags("todo.txt", [flag({ task_id: "a" }), flag({ task_id: "b" })]);
		pendingConflicts.remove("a");
		expect(flagsForPath(get(pendingConflicts), "todo.txt").map((f) => f.task_id)).toEqual(["b"]);
	});

	it("removing an unknown id is a no-op", () => {
		pendingConflicts.clear();
		pendingConflicts.addFlags("todo.txt", [flag({ task_id: "a" })]);
		pendingConflicts.remove("does-not-exist");
		expect(get(pendingConflictCount)).toBe(1);
	});

	it("replaceForPath reconciles the store against a fresh list_conflicts result", () => {
		pendingConflicts.clear();
		pendingConflicts.addFlags("todo.txt", [flag({ task_id: "stale" })]);
		pendingConflicts.replaceForPath("todo.txt", [flag({ task_id: "fresh" })]);
		expect(flagsForPath(get(pendingConflicts), "todo.txt").map((f) => f.task_id)).toEqual([
			"fresh"
		]);
	});

	it("has no dismiss method: dismissal never clears a flag from this store", () => {
		pendingConflicts.clear();
		pendingConflicts.addFlags("todo.txt", [flag({ task_id: "a" })]);
		// Dismissal is UI-only state a component owns for itself; the only ways this store's
		// count can shrink are remove() (a real resolve) and replaceForPath() (a daemon refresh).
		expect("dismiss" in pendingConflicts).toBe(false);
		expect(get(pendingConflictCount)).toBe(1);
	});

	it("flagsForPath sorts by line number for a stable review order", () => {
		pendingConflicts.clear();
		pendingConflicts.addFlags("todo.txt", [
			flag({ task_id: "b", line_number: 9 }),
			flag({ task_id: "a", line_number: 2 })
		]);
		expect(flagsForPath(get(pendingConflicts), "todo.txt").map((f) => f.task_id)).toEqual([
			"a",
			"b"
		]);
	});
});

describe("isResurrectCandidate", () => {
	it("is true when mine is empty/blank and theirs is populated", () => {
		expect(isResurrectCandidate(flag({ mine: "", theirs: "buy milk" }))).toBe(true);
		expect(isResurrectCandidate(flag({ mine: "   ", theirs: "buy milk" }))).toBe(true);
	});

	it("is false for an ordinary same-word conflict (both sides non-empty)", () => {
		expect(isResurrectCandidate(flag({ mine: "buy milk", theirs: "buy milkk" }))).toBe(false);
	});

	it("is false when both sides are empty", () => {
		expect(isResurrectCandidate(flag({ mine: "", theirs: "" }))).toBe(false);
	});
});
