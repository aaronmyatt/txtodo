// Test-only stand-in for `@tauri-apps/api/event` (tasks/desktop-playwright-tests/notes.md), active
// only under `vite dev --mode e2e` (see vite.config.ts's alias and ./core.ts's module doc).
//
// The real `watch`/`daemon-change` pair is a genuine server-push gRPC stream (`Watch`), forwarded
// by the Tauri bridge as Tauri events. `e2e_bridge` (apps/desktop/src-tauri/src/bin/e2e_bridge.rs)
// deliberately doesn't implement that stream — plain request/response HTTP handlers cover the six
// Playwright scenarios with far less surface. Instead, this module polls `get_file`/
// `list_conflicts` for whatever paths the app most recently `watch()`ed and synthesizes a
// `daemon-change` event when something differs. This is still "real reconciliation, not mocks"
// (the notes' own bar): every byte and every conflict flag it observes came from a real `txtodod`
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
interface ReviewFlag {
	task_id: string;
	line_number: number;
	mine: string;
	theirs: string;
}

const POLL_MS = 150;

async function currentHash(path: string): Promise<string | null> {
	try {
		const file = await invoke<FileContents>("get_file", { path });
		return file.hash;
	} catch {
		return null; // the file may not exist yet (e.g. a ref: dir's todo.txt before its first Add)
	}
}

async function currentConflictIds(path: string): Promise<{ ids: Set<string>; flags: ReviewFlag[] }> {
	try {
		const flags = await invoke<ReviewFlag[]>("list_conflicts", { path });
		return { ids: new Set(flags.map((f) => f.task_id)), flags };
	} catch {
		return { ids: new Set(), flags: [] };
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
		const lastConflictIds = new Map<string, Set<string>>();

		const tick = async () => {
			if (stopped) return;
			for (const path of watchedPaths) {
				const [hash, conflicts] = await Promise.all([currentHash(path), currentConflictIds(path)]);
				const seenBefore = lastHash.has(path);
				const prevHash = lastHash.get(path);
				const prevIds = lastConflictIds.get(path) ?? new Set<string>();
				const newFlags = conflicts.flags.filter((f) => !prevIds.has(f.task_id));
				const hashChanged = seenBefore && prevHash !== hash;

				// First observation of a path is a baseline, not a change (a real `Watch` resolves
				// once established; it doesn't fire an initial synthetic event either).
				if (seenBefore && (hashChanged || newFlags.length > 0)) {
					cb({ payload: { path, hash: hash ?? "", ops: [], review: newFlags } as T });
				}
				lastHash.set(path, hash);
				lastConflictIds.set(path, conflicts.ids);
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
