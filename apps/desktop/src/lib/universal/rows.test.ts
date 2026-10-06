import { describe, expect, it } from "vitest";
import type { UniversalTask } from "../daemon";
import { contextCounts, groupRows, stats, toRows } from "./rows";

const task = (over: Partial<UniversalTask>): UniversalTask => ({
	workspace_id: "W1",
	workspace_root: "/home/me/work",
	root_list: "todo.txt",
	line_number: 1,
	priority: null,
	contexts: [],
	description: "",
	done: false,
	completion_date: null,
	due: null,
	projects: [],
	task_id: "",
	ref_progress: null,
	...over
});

const today = "2026-10-06";

describe("Universal rows", () => {
	const rows = toRows(
		[
			task({ line_number: 1, priority: "B", due: "2026-10-04", contexts: ["@phone"] }),
			task({ line_number: 2, priority: "A", due: "2026-10-09", contexts: ["@phone", "@home"] }),
			task({ line_number: 3, contexts: ["@home"] }),
			task({ line_number: 4, done: true, due: "2026-10-01", completion_date: "2026-10-05" }),
			task({ line_number: 5, priority: "A", workspace_id: "W2", workspace_root: "/home/me/home" })
		],
		today
	);

	it("counts the stat strip, done lines never overdue", () => {
		expect(stats(rows)).toEqual({ overdue: 1, soon: 1, open: 4, done: 1 });
	});

	it("groups by priority with no priority last, sorted inside by due then priority", () => {
		const groups = groupRows(rows, "priority", ["work", "home"]);
		expect(groups.map((g) => g.name)).toEqual(["(A)", "(B)", "No priority"]);
		expect(groups[0].rows.map((r) => r.task.line_number)).toEqual([2, 5]);
		expect(groups[2].rows.map((r) => r.task.line_number)).toEqual([3, 4]);
	});

	it("groups by due in calendar order", () => {
		expect(groupRows(rows, "due", []).map((g) => g.name)).toEqual(["Overdue", "This week", "No date"]);
	});

	it("groups by workspace in the given order", () => {
		expect(groupRows(rows, "workspace", ["home", "work"]).map((g) => g.name)).toEqual(["home", "work"]);
	});

	it("counts contexts once per row, most used first", () => {
		expect(contextCounts(rows)).toEqual([
			["@home", 2],
			["@phone", 2]
		]);
	});
});
