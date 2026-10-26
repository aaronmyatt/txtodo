// Test-only stand-in for `@tauri-apps/api/event` (tasks/desktop-playwright-tests/notes.md), active
// only under `vite dev --mode e2e` (see vite.config.ts's alias and ./core.ts's module doc).
//
// The real `watch`/`daemon-change` pair is a genuine server-push gRPC stream (`Watch`), forwarded
// by the Tauri bridge as Tauri events. `e2e_bridge` (apps/desktop/src-tauri/src/bin/e2e_bridge.rs)
// deliberately doesn't implement that stream — plain request/response HTTP handlers cover the six
// Playwright scenarios with far less surface. Instead, this module polls `get_file` for whatever
// paths the app has shown interest in (`./core.ts`'s `watchedPaths`) and synthesizes a
// `daemon-change` event when something differs. This is still "real reconciliation, not mocks"
// (the notes' own bar): every byte it observes came from a real `txtodod`
// acting on a real file — only the *transport* from daemon to browser is polling instead of a
// push stream, which is invisible to the component code under test ($lib/daemon.ts's
// `onDaemonChange` callback looks identical either way).
import { invoke, watchedPaths } from "./core";

type UnlistenFn = () => void;
interface EventPayload<T> {
	payload: T;
}
interface FileContents {
	hash: string;
}
const POLL_MS = 150;

/** The `Change.path` the real daemon uses to announce a layout change (task
 * layout-hot-reload-clients); this shim synthesizes the same event from a `workspace_layout` poll. */
const LAYOUT_CHANGE_PATH = "txtodo.toml";

interface WorkspaceLayout {
	refs_dir: string;
	todo_file: string;
}

async function currentLayout(): Promise<string | null> {
	try {
		const layout = await invoke<WorkspaceLayout>("workspace_layout", {});
		return `${layout.refs_dir}\n${layout.todo_file}`;
	} catch {
		return null;
	}
}

async function currentHash(path: string): Promise<string | null> {
	try {
		const file = await invoke<FileContents>("get_file", { path });
		return file.hash;
	} catch {
		return null; // the file may not exist yet (e.g. a ref: dir's todo.txt before its first Add)
	}
}

/** Mirrors `@tauri-apps/api/event`'s `listen`: https://v2.tauri.app/develop/calling-frontend/ */
export function listen<T>(event: string, cb: (event: EventPayload<T>) => void): Promise<UnlistenFn> {
	if (event === "daemon-status") {
		// e2e_bridge only starts serving once a real daemon answered `Health` — there is no
		// "spawning"/"connecting" phase left to observe by the time a test can reach this.
		cb({ payload: "connected" as T });
		return Promise.resolve(() => {});
	}

	if (event === "daemon-change") {
		let stopped = false;
		const lastHash = new Map<string, string | null>();
		// `undefined` until the first poll: the baseline, never a change (same rule as the files).
		let lastLayout: string | null | undefined;

		const tick = async () => {
			if (stopped) return;
			// The real `Watch` stream carries a `txtodo.toml` change whenever the daemon's layout
			// changes (an RPC set, or a hot reload of the file); here it is a layout poll.
			const layout = await currentLayout();
			if (lastLayout !== undefined && layout !== lastLayout) {
				cb({ payload: { path: LAYOUT_CHANGE_PATH, hash: "", ops: [] } as T });
			}
			lastLayout = layout;
			for (const path of watchedPaths) {
				const hash = await currentHash(path);
				const seenBefore = lastHash.has(path);
				const hashChanged = seenBefore && lastHash.get(path) !== hash;

				// First observation of a path is a baseline, not a change (a real `Watch` resolves
				// once established; it doesn't fire an initial synthetic event either).
				if (hashChanged) {
					cb({ payload: { path, hash: hash ?? "", ops: [] } as T });
				}
				lastHash.set(path, hash);
			}
			if (!stopped) setTimeout(tick, POLL_MS);
		};
		tick();
		return Promise.resolve(() => {
			stopped = true;
		});
	}

	// "quick-add-shown" and anything else: no-op unlisten. Quick-add isn't one of the six
	// Playwright scenarios (tasks/desktop-playwright-tests/notes.md), so nothing here drives it.
	return Promise.resolve(() => {});
}
