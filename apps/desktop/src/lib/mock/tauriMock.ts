// Dev-only in-browser stand-in for the Tauri command bridge. Lets the SvelteKit frontend render
// and be clicked through with `npm run dev` in a plain browser — no Rust daemon, no Tauri webview
// — for fast GUI iteration (design iteration + screenshots). `$lib/tauriShim.ts` only reaches this
// module when `window.__TAURI_INTERNALS__` is absent, so it never runs inside the real app.
// State lives in `./state.ts`, command behaviour in `./logic.ts` — this file is only the
// `invoke(cmd, args)` switch tying the two to the Tauri command names `daemon.ts`/`devices/api.ts`
// call.
import {
	applyMutations,
	mockPairAccept,
	mockPairConfirm,
	mockPairOffer,
	mockTokenCreate,
	mockTokenRevoke,
	type Mutation,
	type TaskRef
} from "./logic";
import { conflicts, delay, files, hashOf, listFilesDto, opLog, setConflicts, tokens } from "./state";

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
			return { path: f.path, text: f.text, hash: hashOf(f) } as T;
		}
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
		default:
			throw new Error(`mock Tauri bridge: unhandled command "${cmd}"`);
	}
}
