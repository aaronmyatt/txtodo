// Unit test for the "workspace loading" retry (task daemon-early-bind): no real timers, no daemon.
import { describe, expect, it } from "vitest";
import { get } from "svelte/store";
import { LOADING_MAX_TRIES, retryWhileLoading } from "../loadingRetry";
import { openingWorkspace } from "../stores/loading";

const loading = () => new Error("code: 'The service is currently unavailable', message: \"workspace loading\"");
const noWait = () => Promise.resolve();

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

	it("gives up after the last try and stops counting itself as waiting", async () => {
		let calls = 0;
		await expect(
			retryWhileLoading(async () => {
				calls++;
				throw loading();
			}, noWait)
		).rejects.toThrow("workspace loading");
		expect(calls).toBe(LOADING_MAX_TRIES);
		expect(get(openingWorkspace)).toBe(0);
	});
});
