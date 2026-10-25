// How the workspace menu groups its rows (task two-kinds-of-workspace, ADR 0043): the lists txtodo
// syncs between your own devices first (the default among them), the repos git syncs after, each
// under a heading. A daemon that predates the kind sends none, and then there is one group with no
// heading, as before. Pure, so a unit test can hold it.
import type { WorkspaceInfo } from "$lib/daemon";

export interface WorkspaceGroup {
	/** The heading, or `null` when the daemon told no kinds and the rows are not grouped. */
	heading: string | null;
	items: WorkspaceInfo[];
}

/** The folder a root ends in: the name a list was made under. */
export function folderOf(root: string): string {
	return root.split(/[\\/]/).filter(Boolean).pop() ?? root;
}

/** `workspaces` as groups, keeping the daemon's order inside each. Empty groups are left out. */
export function groupWorkspaces(workspaces: WorkspaceInfo[]): WorkspaceGroup[] {
	if (!workspaces.some((w) => w.kind === "list" || w.kind === "repo")) {
		return workspaces.length ? [{ heading: null, items: workspaces }] : [];
	}
	const lists = workspaces.filter((w) => w.kind === "list");
	const repos = workspaces.filter((w) => w.kind !== "list");
	const groups: WorkspaceGroup[] = [
		{ heading: "Lists", items: lists },
		{ heading: "Repos", items: repos }
	];
	return groups.filter((g) => g.items.length > 0);
}
