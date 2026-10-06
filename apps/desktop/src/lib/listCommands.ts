// List mode's commands (specs/client-parity.toml list.*): AppShell resolves the key and hands the
// id to whichever buffer listens (the Tasks page's root FileView). A plain listener set, like
// `$lib/escape.ts`: one buffer at a time, nothing to store.

const listeners = new Set<(id: string) => void>();

/** Sends list command `id` to the listening buffer; false when none listens. */
export function runListCommand(id: string): boolean {
	for (const fn of [...listeners]) fn(id);
	return listeners.size > 0;
}

/** Listens for list commands; returns the unsubscribe. */
export function onListCommand(fn: (id: string) => void): () => void {
	listeners.add(fn);
	return () => listeners.delete(fn);
}
