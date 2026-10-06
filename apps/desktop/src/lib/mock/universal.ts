// Mock-only Universal toggles and Undo (`npm run dev` without Tauri). Lenient text edits on the one
// seeded `todo.txt`, with a one-step snapshot per path for Undo; the real rules are the core's.
import { applyMutations, type ApplyResult } from "./logic";
import { files, hashOf } from "./state";

const before = new Map<string, string>();

export function mockUniversalSetDone(args: Record<string, unknown>): ApplyResult {
	const path = args.path as string;
	const n = args.lineNumber as number;
	const f = files.get(path);
	if (!f) throw new Error(`mock daemon: unknown path "${path}"`);
	before.set(path, f.text);
	const line = f.text.split("\n")[n - 1] ?? "";
	const next = args.done ? `x ${args.today as string} ${line}` : line.replace(/^x \d{4}-\d{2}-\d{2} /, "");
	return applyMutations(path, [{ kind: "edit", task: { line_number: n, task_id: "" }, new_line: next }]);
}

export function mockUniversalUndo(args: Record<string, unknown>): ApplyResult {
	const path = args.path as string;
	const f = files.get(path);
	const text = before.get(path);
	if (!f || text === undefined) throw new Error("mock daemon: nothing to undo");
	before.delete(path);
	return applyMutations(path, [{ kind: "replace", base_hash: hashOf(f), contents: text }]);
}
