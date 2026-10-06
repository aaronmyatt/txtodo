// This session's own changes, newest last (specs/client-parity.toml list.undo, toast.undo): what
// `u` and a toast's Undo take back, through the daemon's Undo, all of one change's ops at once.
// Changes from before the session, or from other clients, are not the app's to guess at (the
// TUI's rule, crates/txtodo-tui/src/commands_nav.rs::undo_last).

/** One change as the daemon applied it: `ops` is the `ApplyResult.applied` count. */
export interface SessionChange {
	path: string;
	ops: number;
	/** The workspace it was made in; "" for the open one. */
	workspaceRoot: string;
}

const MAX = 100;
const stack: SessionChange[] = [];

export function recordChange(change: SessionChange): void {
	if (change.ops <= 0) return;
	stack.push(change);
	if (stack.length > MAX) stack.shift();
}

/** The newest change, taken off the stack; undefined when there is none. */
export function popChange(): SessionChange | undefined {
	return stack.pop();
}

/** Takes `change` off the stack only if it is still the newest (a toast's Undo must not undo a
 * later change's ops in its name). */
export function popIfNewest(change: SessionChange): boolean {
	if (stack.at(-1) !== change) return false;
	stack.pop();
	return true;
}

/** The newest change, left on the stack. */
export function newestChange(): SessionChange | undefined {
	return stack.at(-1);
}
