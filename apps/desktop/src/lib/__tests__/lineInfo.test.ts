// Unit tests for the pure todo.txt line-decoration logic ($lib/todotxt/lineInfo.ts). No test
// runner is wired up in apps/desktop/package.json yet (that's tasks/desktop-stack-mapping's
// job) — this file uses the standard Vitest API (https://vitest.dev/api/) so it slots in
// unchanged once `vitest` is added; run it directly with `npx vitest run` in the meantime.
import { describe, expect, it } from "vitest";
import {
	completedLineInfo,
	dirOf,
	findIdTagRanges,
	findRefTag,
	joinPath,
	resolveRefIndicator,
	type FileProgress
} from "../todotxt/lineInfo";

describe("completedLineInfo", () => {
	it("is null for an open task line", () => {
		expect(completedLineInfo("(A) Call Mom @phone")).toBeNull();
	});

	it("is null for a blank line", () => {
		expect(completedLineInfo("")).toBeNull();
	});

	it("finds the description start after one date (completion only)", () => {
		const info = completedLineInfo("x 2011-03-03 Call Mom @phone");
		expect(info).toEqual({ descriptionStart: "x 2011-03-03 ".length });
	});

	it("finds the description start after two dates (completion + creation)", () => {
		const text = "x 2011-03-03 2011-03-01 Call Mom @phone";
		const info = completedLineInfo(text);
		expect(info).toEqual({ descriptionStart: "x 2011-03-03 2011-03-01 ".length });
	});

	it("handles a completed line with no description left to strike", () => {
		const info = completedLineInfo("x 2011-03-03");
		expect(info).toEqual({ descriptionStart: "x 2011-03-03".length });
	});

	it("does not match a description that merely starts with x", () => {
		expect(completedLineInfo("xylophone lessons @home")).toBeNull();
	});
});

describe("findIdTagRanges", () => {
	it("returns no ranges when there is no id: tag", () => {
		expect(findIdTagRanges("(A) Call Mom @phone")).toEqual([]);
	});

	it("locates a single id: tag", () => {
		const prefix = "Call Mom @phone ";
		const tag = "id:01hx5";
		const ranges = findIdTagRanges(prefix + tag);
		expect(ranges).toEqual([{ from: prefix.length, to: prefix.length + tag.length }]);
	});

	it("locates multiple id: tags (defensive — spec expects at most one)", () => {
		const first = "id:aaa";
		const mid = " foo ";
		const second = "id:bbb";
		expect(findIdTagRanges(first + mid + second)).toEqual([
			{ from: 0, to: first.length },
			{ from: first.length + mid.length, to: first.length + mid.length + second.length }
		]);
	});
});

describe("findRefTag", () => {
	it("is null when there is no ref: tag", () => {
		expect(findRefTag("Plan the roadmap +work")).toBeNull();
	});

	it("extracts the slug", () => {
		expect(findRefTag("Plan the roadmap +work ref:q4-roadmap")).toEqual({ slug: "q4-roadmap" });
	});
});

describe("dirOf / joinPath", () => {
	it("dirOf a top-level path is empty", () => {
		expect(dirOf("todo.txt")).toBe("");
	});

	it("dirOf a nested path is its parent directory", () => {
		expect(dirOf("q4-roadmap/todo.txt")).toBe("q4-roadmap");
		expect(dirOf("a/b/todo.txt")).toBe("a/b");
	});

	it("joinPath handles an empty (top-level) directory", () => {
		expect(joinPath("", "q4-roadmap")).toBe("q4-roadmap");
	});

	it("joinPath nests under a non-empty directory", () => {
		expect(joinPath("q4-roadmap", "todo.txt")).toBe("q4-roadmap/todo.txt");
	});
});

describe("resolveRefIndicator", () => {
	function filesMap(entries: FileProgress[]): Map<string, FileProgress> {
		return new Map(entries.map((f) => [f.path, f]));
	}

	it("returns progress counts when the ref directory has a todo.txt", () => {
		const files = filesMap([{ path: "q4-roadmap/todo.txt", done: 2, total: 5 }]);
		expect(resolveRefIndicator("todo.txt", "q4-roadmap", files)).toEqual({
			kind: "progress",
			done: 2,
			total: 5
		});
	});

	it("returns notes when only notes.md exists", () => {
		const files = filesMap([{ path: "q4-roadmap/notes.md", done: 0, total: 0 }]);
		expect(resolveRefIndicator("todo.txt", "q4-roadmap", files)).toEqual({ kind: "notes" });
	});

	it("prefers todo.txt progress over a notes.md in the same directory", () => {
		const files = filesMap([
			{ path: "q4-roadmap/todo.txt", done: 1, total: 3 },
			{ path: "q4-roadmap/notes.md", done: 0, total: 0 }
		]);
		expect(resolveRefIndicator("todo.txt", "q4-roadmap", files)).toEqual({
			kind: "progress",
			done: 1,
			total: 3
		});
	});

	it("is null for a dangling ref (directory not synced)", () => {
		expect(resolveRefIndicator("todo.txt", "ghost", filesMap([]))).toBeNull();
	});

	it("resolves relative to the containing file's own directory, not the workspace root", () => {
		const files = filesMap([{ path: "q4-roadmap/sync-section/todo.txt", done: 0, total: 1 }]);
		expect(resolveRefIndicator("q4-roadmap/todo.txt", "sync-section", files)).toEqual({
			kind: "progress",
			done: 0,
			total: 1
		});
	});
});
