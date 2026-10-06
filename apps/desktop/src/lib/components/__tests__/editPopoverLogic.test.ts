// Tests for the pure logic behind `EditPopover.svelte` (tasks/desktop-edit-popover).
//
// NOTE: no test runner is wired into apps/desktop yet (no vitest/jest in package.json) — this
// file uses vitest's API since that's the standard fit for a Vite/SvelteKit project, but it
// won't run until a runner is added. Left here in the conventional location rather than pulling
// in a whole framework as a drive-by of this task.
import { describe, expect, it } from "vitest";
import {
	decodeUlidTimestampMs,
	formatRelativeTime,
	isNoOpEdit,
	localToday,
	shortDevice
} from "../editPopoverLogic";

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
