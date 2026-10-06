// The Universal page's pure model (task desktop-ui-revamp): rows from `universalTasks()`, the
// stat strip, grouping and the sort inside a group. No DOM, so Vitest covers it.
import type { UniversalTask } from "../daemon";
import { dueLabel, type DueLabel } from "../todotxt/due";

export type GroupKind = "priority" | "due" | "project" | "context" | "workspace";

export const GROUP_KINDS: { kind: GroupKind; label: string }[] = [
	{ kind: "priority", label: "Priority" },
	{ kind: "due", label: "Due" },
	{ kind: "project", label: "Project" },
	{ kind: "context", label: "Context" },
	{ kind: "workspace", label: "Workspace" }
];

const DUE_ORDER = ["Overdue", "Today", "This week", "Later", "No date"];

export interface Row {
	task: UniversalTask;
	/** The workspace's short name (last path segment). */
	ws: string;
	due: DueLabel | null;
	/** Stable across refetches: workspace plus line. */
	key: string;
}

/** The row as a todo.txt line for the search matcher (`query.ts::matchesQuery`): the `x` and
 * priority back in front of the description, so `is:done` and `(A)` match as in the buffer. */
export function matchLine(task: UniversalTask): string {
	if (task.done) return `x ${task.completion_date ? `${task.completion_date} ` : ""}${task.description}`;
	return task.priority ? `(${task.priority}) ${task.description}` : task.description;
}

/** Whether the query asks for done lines (`is:done`), which shows them even with show-done off. */
export function wantsDone(query: string): boolean {
	return /(^|\s)is:done(\s|$)/i.test(query);
}

/** Last path segment of an absolute root, for a compact label; the full path if none. */
export function workspaceLabel(root: string): string {
	return root.split(/[/\\]/).filter(Boolean).pop() ?? root;
}

export function toRows(tasks: UniversalTask[], today: string): Row[] {
	return tasks.map((task) => ({
		task,
		ws: workspaceLabel(task.workspace_root),
		due: task.done ? null : dueLabel(task.due, today),
		key: `${task.workspace_id}:${task.line_number}`
	}));
}

export interface Stats {
	overdue: number;
	/** Due today through 7 days out. */
	soon: number;
	open: number;
	done: number;
}

export function stats(rows: Row[]): Stats {
	const open = rows.filter((r) => !r.task.done);
	return {
		overdue: open.filter((r) => r.due && r.due.days < 0).length,
		soon: open.filter((r) => r.due && r.due.days >= 0 && r.due.days <= 7).length,
		open: open.length,
		done: rows.length - open.length
	};
}

/** The group a row falls in: its first project or context, by the mockup's rules. */
export function groupOf(kind: GroupKind, r: Row): string {
	switch (kind) {
		case "priority":
			return r.task.priority ? `(${r.task.priority})` : "No priority";
		case "due":
			if (!r.due) return "No date";
			if (r.due.days < 0) return "Overdue";
			if (r.due.days === 0) return "Today";
			return r.due.days <= 7 ? "This week" : "Later";
		case "project":
			return r.task.projects[0] ?? "No project";
		case "context":
			return r.task.contexts[0] ?? "No context";
		case "workspace":
			return r.ws;
	}
}

function groupRank(kind: GroupKind, name: string, wsOrder: string[]): number {
	if (kind === "due") return DUE_ORDER.indexOf(name);
	if (kind === "workspace") return wsOrder.indexOf(name);
	if (kind === "priority") return name.startsWith("(") ? name.charCodeAt(1) : 999;
	return name.startsWith("No ") ? 9999 : 0;
}

/** In a group: not done first, then by due date, priority, line. */
export function compareRows(a: Row, b: Row): number {
	const rank = (r: Row) =>
		(r.task.done ? 1e7 : 0) +
		(r.due ? r.due.days + 1000 : 5000) * 100 +
		(r.task.priority ? r.task.priority.charCodeAt(0) - 64 : 27);
	return rank(a) - rank(b) || a.task.line_number - b.task.line_number || a.ws.localeCompare(b.ws);
}

export interface Group {
	name: string;
	rows: Row[];
}

export function groupRows(rows: Row[], kind: GroupKind, wsOrder: string[]): Group[] {
	const buckets = new Map<string, Row[]>();
	for (const r of rows) {
		const g = groupOf(kind, r);
		const bucket = buckets.get(g);
		if (bucket) bucket.push(r);
		else buckets.set(g, [r]);
	}
	return [...buckets.entries()]
		.sort(([a], [b]) => groupRank(kind, a, wsOrder) - groupRank(kind, b, wsOrder) || a.localeCompare(b))
		.map(([name, list]) => ({ name, rows: list.sort(compareRows) }));
}

/** Context chips for the rows in scope: most used first, then by name. */
export function contextCounts(rows: Row[]): [string, number][] {
	const counts = new Map<string, number>();
	for (const r of rows) for (const c of new Set(r.task.contexts)) counts.set(c, (counts.get(c) ?? 0) + 1);
	return [...counts.entries()].sort((a, b) => b[1] - a[1] || a[0].localeCompare(b[0]));
}
