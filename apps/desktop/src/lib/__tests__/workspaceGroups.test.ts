// How the workspace menu groups its rows (task two-kinds-of-workspace), as the pure decision
// `WorkspaceSwitcher.svelte` reads rather than by mounting the component.
import { describe, expect, it } from "vitest";
import type { WorkspaceInfo } from "../daemon";
import { folderOf, groupWorkspaces } from "../workspaceGroups";

function ws(id: string, root: string, kind?: WorkspaceInfo["kind"]): WorkspaceInfo {
	return {
		id,
		root,
		added_at_ms: 0,
		root_exists: true,
		has_state: true,
		load_state: "ready",
		load_error: "",
		kind
	};
}

describe("groupWorkspaces", () => {
	it("puts lists first and repos after, each under a heading, keeping the daemon's order", () => {
		const groups = groupWorkspaces([
			ws("1", "/home/a/txtodo", "repo"),
			ws("2", "/d/default", "list"),
			ws("3", "/d/lists/work", "list"),
			ws("4", "/home/a/givens", "repo")
		]);
		expect(groups.map((g) => g.heading)).toEqual(["Lists", "Repos"]);
		expect(groups[0].items.map((w) => w.id)).toEqual(["2", "3"]);
		expect(groups[1].items.map((w) => w.id)).toEqual(["1", "4"]);
	});

	it("leaves out a group with nothing in it", () => {
		const onlyLists = groupWorkspaces([ws("1", "/d/default", "list")]);
		expect(onlyLists.map((g) => g.heading)).toEqual(["Lists"]);
		const onlyRepos = groupWorkspaces([ws("1", "/home/a/txtodo", "repo")]);
		expect(onlyRepos.map((g) => g.heading)).toEqual(["Repos"]);
	});

	it("does not group when the daemon told no kinds: one group, no heading", () => {
		const rows = [ws("1", "/a"), ws("2", "/b", "")];
		expect(groupWorkspaces(rows)).toEqual([{ heading: null, items: rows }]);
		expect(groupWorkspaces([])).toEqual([]);
	});

	it("a mirror of a peer's workspace has no list kind and sits with the repos", () => {
		const groups = groupWorkspaces([ws("1", "/d/lists/work", "list"), ws("2", "/d/remote/01X", "repo")]);
		expect(groups[1].items.map((w) => w.id)).toEqual(["2"]);
	});
});

describe("folderOf", () => {
	it("is the last part of the root, whichever way it is separated", () => {
		expect(folderOf("/d/lists/work")).toBe("work");
		expect(folderOf("C:\\Users\\a\\lists\\work")).toBe("work");
		expect(folderOf("/d/lists/work/")).toBe("work");
		expect(folderOf("")).toBe("");
	});
});
