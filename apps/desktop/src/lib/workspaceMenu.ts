// `W` opens the header's workspace menu (specs/client-parity.toml nav.workspace_menu): AppShell
// resolves the key, the menu listens. A plain listener set, like `$lib/listCommands.ts`.

const listeners = new Set<() => void>();

export function requestWorkspaceMenu(): void {
	for (const fn of [...listeners]) fn();
}

export function onWorkspaceMenuRequest(fn: () => void): () => void {
	listeners.add(fn);
	return () => listeners.delete(fn);
}
