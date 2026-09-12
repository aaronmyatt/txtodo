import { describe, expect, it } from "vitest";
import { relativeTime } from "./time";

const NOW = 1_000_000_000_000; // arbitrary fixed instant

describe("relativeTime", () => {
	it("reports just now under a minute", () => {
		expect(relativeTime(NOW - 30_000, NOW)).toBe("just now");
		expect(relativeTime(NOW, NOW)).toBe("just now");
	});

	it("reports minutes", () => {
		expect(relativeTime(NOW - 5 * 60_000, NOW)).toBe("5m ago");
	});

	it("reports hours", () => {
		expect(relativeTime(NOW - 3 * 3_600_000, NOW)).toBe("3h ago");
	});

	it("reports days", () => {
		expect(relativeTime(NOW - 2 * 86_400_000, NOW)).toBe("2d ago");
	});

	it("falls back to an ISO date beyond 30 days", () => {
		const atMs = NOW - 40 * 86_400_000;
		expect(relativeTime(atMs, NOW)).toBe(new Date(atMs).toISOString().slice(0, 10));
	});

	it("never reports a negative delta for a clock-skewed future timestamp", () => {
		expect(relativeTime(NOW + 5_000, NOW)).toBe("just now");
	});
});
