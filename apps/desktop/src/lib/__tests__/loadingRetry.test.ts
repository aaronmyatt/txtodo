// Unit test for the "workspace loading" retry (task daemon-early-bind): no real timers, no daemon.
import { describe, expect, it } from "vitest";
import { get } from "svelte/store";
import { LOADING_DEADLINE_MS, LOADING_RETRY_MS, retryWhileLoading, type RetryClock } from "../loadingRetry";
import { openingWorkspace } from "../stores/loading";

const loading = () => new Error("code: 'The service is currently unavailable', message: \"workspace loading\"");

/** A clock that only moves when the test (or a sleep) moves it. */
function fakeClock(): RetryClock & { advance: (ms: number) => void } {
	let t = 0;
	return {
		now: () => t,
		sleep: (ms) => {
			t += ms;
			return Promise.resolve();
		},
		advance: (ms) => {
			t += ms;
		}
	};
}
const noWait = fakeClock();

describe("retryWhileLoading", () => {
	it("retries a loading refusal until the call succeeds, counting itself as waiting meanwhile", async () => {
		let calls = 0;
		const seen: number[] = [];
		const result = await retryWhileLoading(async () => {
			seen.push(get(openingWorkspace));
			calls++;
			if (calls < 3) throw loading();
			return "contents";
		}, noWait);
		expect(result).toBe("contents");
		expect(calls).toBe(3);
		expect(seen).toEqual([0, 1, 1]); // waiting from the first refusal on
		expect(get(openingWorkspace)).toBe(0);
	});

	it("throws any other error at once, without retrying", async () => {
		let calls = 0;
		await expect(
			retryWhileLoading(async () => {
				calls++;
				throw new Error("no document nope.txt");
			}, noWait)
		).rejects.toThrow("no document");
		expect(calls).toBe(1);
		expect(get(openingWorkspace)).toBe(0);
	});

	it("gives up at the deadline and stops counting itself as waiting", async () => {
		const clock = fakeClock();
		let calls = 0;
		await expect(
			retryWhileLoading(async () => {
				calls++;
				throw loading();
			}, clock)
		).rejects.toThrow("workspace loading");
		// Instant refusals: one try per retry interval, the last one at the deadline itself.
		expect(calls).toBe(LOADING_DEADLINE_MS / LOADING_RETRY_MS + 1);
		expect(clock.now()).toBeLessThanOrEqual(LOADING_DEADLINE_MS);
		expect(get(openingWorkspace)).toBe(0);
	});

	// Code review 2026-09-20, finding 6: a try can block in the daemon for about two minutes.
	it("counts the time a slow try spends in the daemon toward the deadline", async () => {
		const clock = fakeClock();
		let calls = 0;
		await expect(
			retryWhileLoading(async () => {
				calls++;
				clock.advance(120_000); // the daemon held this call for its own wait bound
				throw loading();
			}, clock)
		).rejects.toThrow("workspace loading");
		expect(calls).toBe(3); // 2 min + 1 s, twice, then the third try ends past five minutes
		expect(clock.now()).toBeLessThan(LOADING_DEADLINE_MS + 121_000);
	});
});
