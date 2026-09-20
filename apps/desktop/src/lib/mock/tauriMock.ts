// Dev-only in-browser stand-in for the Tauri command bridge. Lets the SvelteKit frontend render
// and be clicked through with `npm run dev` in a plain browser — no Rust daemon, no Tauri webview
// — for fast GUI iteration (design iteration + screenshots). `$lib/tauriShim.ts` only reaches this
// module when `window.__TAURI_INTERNALS__` is absent, so it never runs inside the real app.
// State lives in `./state.ts`, command behaviour in `./logic.ts` — this file is only the
// `invoke(cmd, args)` switch tying the two to the Tauri command names `daemon.ts`/`devices/api.ts`
// call.
import {
	applyMutations,
	mockEditNotes,
	mockGetNotes,
	mockPairAccept,
	mockPairConfirm,
	mockPairOffer,
	mockTokenCreate,
	mockTokenRevoke,
	type Mutation,
	type TaskRef
} from "./logic";
import {
	conflicts,
	currentWorkspaceRoot,
	delay,
	fakeUlid,
	files,
	hashOf,
	listFilesDto,
	NOW_MS,
	opLog,
	setConflicts,
	setCurrentWorkspaceRoot,
	tokens,
	universalTasksDto,
	workspaces
} from "./state";

export { mockListen } from "./state";

export async function mockInvoke<T>(cmd: string, args?: Record<string, unknown>): Promise<T> {
	await delay(120); // feels like a round-trip, not instant — closer to the real daemon connection
	switch (cmd) {
		case "daemon_status":
		case "retry_connect":
			return "connected" as T;
		case "list_files":
			return listFilesDto() as T;
		case "get_file": {
			const path = args?.path as string;
			const f = files.get(path);
			if (!f) throw new Error(`mock daemon: unknown path "${path}"`);
			return { path: f.path, text: f.text, hash: hashOf(f), task_ids: [] } as T;
		}
		case "build_info":
			// The mock daemon is always "this build": no mismatch banner in mock mode.
			return { version: "0.0.2", release_date: "2026-09-20", daemon_version: "0.0.2", daemon_release_date: "2026-09-20" } as T;
		case "apply":
			return applyMutations(args?.path as string, args?.mutations as Mutation[]) as unknown as T;
		case "history":
			return { ops: opLog.slice(0, (args?.limit as number) ?? opLog.length) } as T;
		case "watch":
			return undefined as T;
		case "list_conflicts": {
			const path = args?.path as string;
			return (path === "todo.txt" ? conflicts.map((c) => ({ ...c })) : []) as T;
		}
		case "resolve": {
			const task = args?.task as TaskRef;
			const path = args?.path as string;
			setConflicts(conflicts.filter((c) => c.task_id !== task.task_id || c.line_number !== task.line_number));
			return applyMutations(path, []) as unknown as T;
		}
		case "pair_offer":
			return mockPairOffer() as T;
		case "pair_accept":
			return mockPairAccept() as T;
		case "pair_confirm_sas":
			return mockPairConfirm() as T;
		case "token_create":
			return mockTokenCreate(args) as T;
		case "token_list":
			return tokens.map((t) => ({ ...t, secret: "" })) as T;
		case "token_revoke":
			return mockTokenRevoke(args) as unknown as T;
		case "op_log":
			return opLog.map((e) => ({ principal: e.principal, op: e.summary, at_ms: e.at_ms })) as T;
		case "get_notes":
			return mockGetNotes((args?.task as TaskRef).task_id) as T;
		case "edit_notes":
			return mockEditNotes((args?.task as TaskRef).task_id, args?.newText as string) as unknown as T;
		case "workspace_root":
			return currentWorkspaceRoot as T;
		case "list_workspaces":
			return workspaces.map((w) => ({ ...w })) as T;
		case "add_workspace": {
			const root = (args?.root as string).trim();
			let ws = workspaces.find((w) => w.root === root);
			if (!ws) {
				ws = { id: fakeUlid(), root, added_at_ms: NOW_MS, root_exists: true, has_state: false, load_state: "ready", load_error: "" };
				workspaces.push(ws);
			}
			return { ...ws } as T;
		}
		case "remove_workspace": {
			const id = args?.id as string;
			const idx = workspaces.findIndex((w) => w.id === id);
			if (idx >= 0) workspaces.splice(idx, 1);
			// Doesn't reproduce the real daemon's idempotent-upsert semantics (an already-removed
			// but once-known id there still returns true) — this mock only backs GUI iteration, not
			// a fixture for testing that distinction.
			return (idx >= 0) as T;
		}
		case "switch_workspace":
			setCurrentWorkspaceRoot(args?.root as string);
			return undefined as T;
		case "universal_tasks":
			return universalTasksDto() as T;
		case "set_main_popover_dirty":
			return undefined as T;
		case "set_pinned":
			// No real OS window to pin in the mock/browser preview (task desktop-always-on) —
			// the store still applies and persists the preference locally either way.
			return undefined as T;
		default:
			throw new Error(`mock Tauri bridge: unhandled command "${cmd}"`);
	}
}
