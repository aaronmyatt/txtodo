// List mode's commands (specs/client-parity.toml list.*): AppShell resolves the key and hands the
// id on. Plain listeners (the sync indicator's `sync.open`) all hear it; of the list buffers
// (FileViews) only the active one does, so a root list and an open detail panel's sub-list never
// both act on one key (task desktop-list-parity). Like `$lib/escape.ts`: nothing to store.

const listeners = new Set<(id: string) => void>();
// List buffers, oldest first; the last one is the active list. A new buffer starts active (a
// sub-list mounts when its detail opens: the human moved into it), a click makes one active again,
// and releasing the active one hands the keys back to the one before it.
const buffers: Buffer[] = [];

/** Sends list command `id` to every listener and the active buffer; false when none listens. */
export function runListCommand(id: string): boolean {
	for (const fn of [...listeners]) fn(id);
	const active = buffers.at(-1);
	active?.fn(id);
	return listeners.size > 0 || active !== undefined;
}

/** Listens for list commands; returns the unsubscribe. */
export function onListCommand(fn: (id: string) => void): () => void {
	listeners.add(fn);
	return () => listeners.delete(fn);
}

type Buffer = { fn: (id: string) => void; onActive?: (active: boolean) => void };

/** Tells the old and the new active buffer when the last one changed. */
function settle(before: Buffer | undefined) {
	const after = buffers.at(-1);
	if (after === before) return;
	before?.onActive?.(false);
	after?.onActive?.(true);
}

/** A list buffer: `fn` hears list commands while it is the active one, which it is from now until
 * another buffer registers or activates. `activate` takes the keys back; `release` drops it.
 * `onActive` hears each change, so the buffer can show whether it has the keys. */
export function listBuffer(
	fn: (id: string) => void,
	onActive?: (active: boolean) => void
): { activate: () => void; release: () => void } {
	const me: Buffer = { fn, onActive };
	const drop = () => {
		const at = buffers.indexOf(me);
		if (at >= 0) buffers.splice(at, 1);
	};
	const before = buffers.at(-1);
	buffers.push(me);
	settle(before);
	return {
		activate: () => {
			const was = buffers.at(-1);
			drop();
			buffers.push(me);
			settle(was);
		},
		release: () => {
			const was = buffers.at(-1);
			drop();
			if (was === me) settle(me);
		}
	};
}
