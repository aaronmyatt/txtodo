// Retries a daemon call the daemon refused only because the workspace it names is still being
// opened (task daemon-early-bind): the answer is `Unavailable` with the message "workspace
// loading", it changed nothing, and the open finishes on its own, so waiting and asking again is
// correct. Any other error is the caller's, and is thrown at once.
import { openingWorkspace } from "./stores/loading";

/** The daemon's exact message for a workspace that is queued or loading. */
const LOADING_MESSAGE = "workspace loading";

/** One second between tries, five minutes at most: past the daemon's own wait bound several times over. */
export const LOADING_RETRY_MS = 1000;
export const LOADING_MAX_TRIES = 300;

export function isWorkspaceLoading(error: unknown): boolean {
	return String(error).includes(LOADING_MESSAGE);
}

const realSleep = (ms: number) => new Promise<void>((resolve) => setTimeout(resolve, ms));

/** Runs `call`, retrying while it fails with "workspace loading"; `openingWorkspace` counts this
 * call as waiting for as long as it is retrying. `sleep` is injectable for tests. */
export async function retryWhileLoading<T>(call: () => Promise<T>, sleep: (ms: number) => Promise<void> = realSleep): Promise<T> {
	let waiting = false;
	try {
		for (let attempt = 0; ; attempt++) {
			try {
				return await call();
			} catch (e) {
				if (!isWorkspaceLoading(e) || attempt + 1 >= LOADING_MAX_TRIES) throw e;
				if (!waiting) {
					waiting = true;
					openingWorkspace.update((n) => n + 1);
				}
				await sleep(LOADING_RETRY_MS);
			}
		}
	} finally {
		if (waiting) openingWorkspace.update((n) => n - 1);
	}
}
