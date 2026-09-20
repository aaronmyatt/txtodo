// Retries a daemon call the daemon refused only because the workspace it names is still being
// opened (task daemon-early-bind): the answer is `Unavailable` with the message "workspace
// loading", it changed nothing, and the open finishes on its own, so waiting and asking again is
// correct. Any other error is the caller's, and is thrown at once.
import { openingWorkspace } from "./stores/loading";

/** The daemon's exact message for a workspace that is queued or loading. */
const LOADING_MESSAGE = "workspace loading";

/** One second between tries, five minutes in all. The bound is a deadline, not a count of tries
 * (code review 2026-09-20, finding 6): one try can sit in the daemon for its own wait bound, about
 * two minutes, so 300 tries was about ten hours, not the five minutes it was meant to be. */
export const LOADING_RETRY_MS = 1000;
export const LOADING_DEADLINE_MS = 5 * 60 * 1000;

export function isWorkspaceLoading(error: unknown): boolean {
	return String(error).includes(LOADING_MESSAGE);
}

const realSleep = (ms: number) => new Promise<void>((resolve) => setTimeout(resolve, ms));

/** Time source and sleep, injectable so a test needs no real timers.
 * performance.now(): https://developer.mozilla.org/en-US/docs/Web/API/Performance/now (monotonic:
 * a wall-clock change mid-wait must not stretch or cut the deadline). */
export interface RetryClock {
	now: () => number;
	sleep: (ms: number) => Promise<void>;
}

const realClock: RetryClock = { now: () => performance.now(), sleep: realSleep };

/** Runs `call`, retrying while it fails with "workspace loading" until `LOADING_DEADLINE_MS` has
 * passed since the first try; `openingWorkspace` counts this call as waiting for as long as it is
 * retrying. The time a try spends inside the daemon counts toward the deadline. */
export async function retryWhileLoading<T>(call: () => Promise<T>, clock: RetryClock = realClock): Promise<T> {
	let waiting = false;
	const giveUpAt = clock.now() + LOADING_DEADLINE_MS;
	const sleep = clock.sleep;
	try {
		for (;;) {
			try {
				return await call();
			} catch (e) {
				if (!isWorkspaceLoading(e) || clock.now() + LOADING_RETRY_MS > giveUpAt) throw e;
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
