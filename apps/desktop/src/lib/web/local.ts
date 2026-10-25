// The commands the browser answers without the Worker host (`vite dev --mode web`, ADR 0042):
// what the desktop asks Tauri about the app, the window and other devices. Plain values, the
// shapes `src-tauri`'s DTOs send (mirrored in `$lib/daemon.ts`), so the same screens render.
// The list commands go to `txtodo-web` in the Worker instead (`HOST_COMMANDS`).

/** Answered by `txtodo-web`'s `Host::call` in the Worker (`crates/txtodo-web/src/host.rs`). */
export const HOST_COMMANDS = new Set(["list_files", "get_file", "apply", "history", "list_duplicates", "watch", "undo_change"]);

/** The one workspace a browser holds for now: no folder stands behind it. */
export const WEB_ROOT = "browser";

/** Answers for commands that need no host; `undefined` (the function, not the value) means none. */
const LOCAL: Record<string, (args: Record<string, unknown>) => unknown> = {
	daemon_status: () => "connected",
	retry_connect: () => "connected",
	build_info: () => ({ version: "web", release_date: "", daemon_version: "web", daemon_release_date: "" }),
	check_updates: () => ({ current: "web", latest: "web", newer: false, command: "" }),
	skill_hint: () => false,
	workspace_root: () => WEB_ROOT,
	workspace_layout: () => ({ refs_dir: "tasks", todo_file: "todo.txt", note: "", outside_refs_dir: [] }),
	list_workspaces: () => [
		{
			id: "browser",
			root: WEB_ROOT,
			added_at_ms: 0,
			root_exists: true,
			has_state: true,
			load_state: "ready",
			load_error: "",
			is_default: true,
			is_remote: false,
			name: "Browser"
		}
	],
	switch_workspace: () => undefined,
	sync_status: () => ({ protocol: 5, pending_ops: 0, peers: [] }),
	universal_tasks: () => [],
	op_log_all: () => [],
	offers_problem: () => ({ problem: "", age_ms: 0 }),
	workspace_offers: () => [],
	device_list: () => [],
	group_status: () => ({ dropped_from_group: false }),
	token_list: () => [],
	// The window and app plumbing a browser tab does not have: accepted and ignored.
	ui_log: () => undefined,
	set_main_popover_dirty: () => undefined,
	set_pinned: () => undefined,
	set_change_notices: () => undefined,
	relaunch: () => undefined
};

/** Whether `cmd` has a local answer. */
export function isLocal(cmd: string): boolean {
	return Object.hasOwn(LOCAL, cmd);
}

/** The local answer to `cmd`. Throws for a command neither this table nor the host answers, with
 * a message the page can show (a toast, a settings row) instead of a silent hang. */
export function answerLocally<T>(cmd: string, args: Record<string, unknown> = {}): T {
	const answer = LOCAL[cmd];
	if (!answer) throw new Error(`"${cmd}" is not available in the browser yet`);
	return answer(args) as T;
}
