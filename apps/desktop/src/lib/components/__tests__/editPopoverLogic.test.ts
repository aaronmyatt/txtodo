// Tests for the pure logic behind `EditPopover.svelte` (tasks/desktop-edit-popover).
//
// NOTE: no test runner is wired into apps/desktop yet (no vitest/jest in package.json) — this
// file uses vitest's API since that's the standard fit for a Vite/SvelteKit project, but it
// won't run until a runner is added. Left here in the conventional location rather than pulling
// in a whole framework as a drive-by of this task.
import { describe, expect, it } from "vitest";
import {
	applyChip,
	decodeUlidTimestampMs,
	formatRelativeTime,
	isNoOpEdit,
	localToday,
	shortDevice,
	toggleComplete
} from "../editPopoverLogic";

describe("applyChip: priority chips", () => {
	it("adds a priority to a line that has none", () => {
		expect(applyChip("call mum", 0, "A", "2026-09-12")).toEqual({
			text: "(A) call mum",
			caret: 4
		});
	});

	it("removes the priority when the same chip is tapped again", () => {
		expect(applyChip("(A) call mum", 0, "A", "2026-09-12")).toEqual({
			text: "call mum",
			caret: 0
		});
	});

	it("replaces one priority with another, keeping the caret's relative offset", () => {
		expect(applyChip("(B) call mum", 5, "A", "2026-09-12")).toEqual({
			text: "(A) call mum",
			caret: 5
		});
	});

	it("removes a bare priority with no trailing description", () => {
		expect(applyChip("(A)", 0, "A", "2026-09-12")).toEqual({ text: "", caret: 0 });
	});
});

describe("applyChip: insert-at-caret chips (no double-spacing)", () => {
	it("does not add a leading space when the caret is already at a word boundary", () => {
		expect(applyChip("call mum ", 9, "+", "2026-09-12")).toEqual({
			text: "call mum +",
			caret: 10
		});
	});

	it("adds exactly one leading space when the caret is mid/after a word", () => {
		expect(applyChip("call mum", 8, "+", "2026-09-12")).toEqual({
			text: "call mum +",
			caret: 10
		});
	});

	it("inserts inside the line, not just at the end", () => {
		expect(applyChip("call mum", 4, "@", "2026-09-12")).toEqual({
			text: "call @ mum",
			caret: 6
		});
	});

	it("skips the leading space when inserting right after an existing space", () => {
		expect(applyChip("call  mum", 5, "due:", "2026-09-12")).toEqual({
			text: "call due: mum",
			caret: 9
		});
	});
});

describe("applyChip: x (completion toggle)", () => {
	it("delegates to toggleComplete and puts the caret at the end", () => {
		const today = "2026-09-12";
		const result = applyChip("call mum", 0, "x", today);
		expect(result.text).toBe(toggleComplete("call mum", today));
		expect(result.caret).toBe(result.text.length);
	});
});

describe("toggleComplete", () => {
	it("completes a plain line: prepends x <today>", () => {
		expect(toggleComplete("call mum", "2026-09-12")).toBe("x 2026-09-12 call mum");
	});

	it("uncompletes back to the exact original plain line", () => {
		const completed = toggleComplete("call mum", "2026-09-12");
		expect(toggleComplete(completed, "2026-09-12")).toBe("call mum");
	});

	it("round-trips a prioritised, dated line through pri: preservation (core-complete-pri)", () => {
		const original = "(A) 2026-09-01 call mum +home";
		const completed = toggleComplete(original, "2026-09-12");
		expect(completed).toBe("x 2026-09-12 2026-09-01 call mum +home pri:A");
		expect(toggleComplete(completed, "2026-09-12")).toBe(original);
	});
});

describe("isNoOpEdit", () => {
	it("is true only for an unchanged line", () => {
		expect(isNoOpEdit("call mum", "call mum")).toBe(true);
		expect(isNoOpEdit("call mum", "call dad")).toBe(false);
	});
});

describe("localToday", () => {
	it("formats a local Date as YYYY-MM-DD", () => {
		expect(localToday(new Date(2026, 8, 12))).toBe("2026-09-12");
	});
});

describe("decodeUlidTimestampMs", () => {
	it("decodes an all-zero prefix as 0", () => {
		expect(decodeUlidTimestampMs("0000000000" + "0".repeat(16))).toBe(0);
	});

	it("decodes the last prefix char as the units place", () => {
		expect(decodeUlidTimestampMs("0000000001" + "0".repeat(16))).toBe(1);
	});

	it("carries into the next base-32 digit correctly", () => {
		// "10" as the last two prefix chars is 1*32 + 0 = 32.
		expect(decodeUlidTimestampMs("0000000010" + "0".repeat(16))).toBe(32);
	});
});

describe("formatRelativeTime", () => {
	const now = Date.parse("2026-09-12T12:00:00Z");

	it("reports well under a minute as just now", () => {
		expect(formatRelativeTime(now - 10_000, now)).toBe("just now");
	});

	it("reports minutes", () => {
		expect(formatRelativeTime(now - 3 * 60_000, now)).toBe("3m ago");
	});

	it("reports hours", () => {
		expect(formatRelativeTime(now - 5 * 3_600_000, now)).toBe("5h ago");
	});

	it("reports days", () => {
		expect(formatRelativeTime(now - 2 * 86_400_000, now)).toBe("2d ago");
	});
});

describe("shortDevice", () => {
	it("shortens a full ULID device id", () => {
		expect(shortDevice("01ARZ3NDEKTSV4RRFFQ69G5FAV")).toBe("01ARZ3ND");
	});

	it("leaves a short id untouched", () => {
		expect(shortDevice("abc")).toBe("abc");
	});
});
