import { describe, expect, it } from "vitest";
import { daysBetween, dueLabel } from "../due";

describe("dueLabel", () => {
	const today = "2026-10-06";
	it("names overdue, today, tomorrow, this week and later", () => {
		expect(dueLabel("2026-10-04", today)).toEqual({ text: "overdue 2d", tone: "overdue", days: -2 });
		expect(dueLabel("2026-10-06", today)?.text).toBe("today");
		expect(dueLabel("2026-10-07", today)?.text).toBe("tomorrow");
		expect(dueLabel("2026-10-13", today)).toEqual({ text: "in 7d", tone: "soon", days: 7 });
		expect(dueLabel("2026-10-15", today)).toEqual({ text: "Oct 15", tone: "later", days: 9 });
	});

	it("is null for no date or a bad one", () => {
		expect(dueLabel(null, today)).toBeNull();
		expect(dueLabel("next-week", today)).toBeNull();
	});

	it("counts across a DST change and a year end in whole days", () => {
		expect(daysBetween("2026-10-24", "2026-10-26")).toBe(2);
		expect(daysBetween("2026-12-31", "2027-01-01")).toBe(1);
	});
});
