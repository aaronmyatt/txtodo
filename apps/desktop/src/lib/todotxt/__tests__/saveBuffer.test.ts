// Vitest: https://vitest.dev/api/
import { describe, expect, it } from "vitest";
import type { ApplyResult, Mutation } from "$lib/daemon";
import { FAILED_PRECONDITION_TOKEN, isReorderOnly, isStaleBase, matchEndings, saveBuffer } from "../saveBuffer";

const OK: ApplyResult = { applied: 1, hash: "newhash", hlc_wall_ms: 1, hlc_counter: 0 };

/** A fake `apply` that records every call and fails the calls `refuse` picks. */
function fakeApply(refuse: (mutations: Mutation[]) => string | null = () => null) {
	const calls: Mutation[][] = [];
	const apply = async (_path: string, mutations: Mutation[]): Promise<ApplyResult> => {
		calls.push(mutations);
		const error = refuse(mutations);
		if (error !== null) throw error;
		return OK;
	};
	return { calls, apply };
}

const isReplace = (mutations: Mutation[]) => mutations[0]?.kind === "replace";

describe("isReorderOnly", () => {
	it("is true for the same lines in another order", () => {
		expect(isReorderOnly("a\nb\nc\n", "b\na\nc\n")).toBe(true);
	});

	it("is false for an unchanged buffer, typed text, or a removed line", () => {
		expect(isReorderOnly("a\nb\n", "a\nb\n")).toBe(false);
		expect(isReorderOnly("a\nb\n", "a\nc\n")).toBe(false);
		expect(isReorderOnly("a\nb\n", "b\n")).toBe(false);
	});

	it("counts duplicate lines, not just the set of lines", () => {
		expect(isReorderOnly("a\na\nb\n", "a\nb\nb\n")).toBe(false);
	});
});

describe("matchEndings", () => {
	it("gives a CRLF baseline its CRLF back and leaves an LF one alone", () => {
		expect(matchEndings("a\r\nb\r\n", "b\na\n")).toBe("b\r\na\r\n");
		expect(matchEndings("a\nb\n", "b\na\n")).toBe("b\na\n");
	});

	it("does not double a CRLF that is already there", () => {
		expect(matchEndings("a\r\nb\r\n", "b\r\na\n")).toBe("b\r\na\r\n");
	});
});

describe("saveBuffer", () => {
	it("sends nothing for an unchanged buffer", async () => {
		const { calls, apply } = fakeApply();
		expect(await saveBuffer(apply, "todo.txt", { text: "a\n", hash: "h" }, "a\n")).toEqual({ how: "none" });
		expect(calls).toEqual([]);
	});

	it("saves a reorder as one Replace against the baseline hash", async () => {
		const { calls, apply } = fakeApply();
		const outcome = await saveBuffer(apply, "todo.txt", { text: "a\nb\n", hash: "h0" }, "b\na\n");
		expect(outcome).toEqual({ how: "replace", hash: "newhash" });
		expect(calls).toEqual([[{ kind: "replace", base_hash: "h0", contents: "b\na\n" }]]);
	});

	it("falls back to the per-line delta when the base is stale", async () => {
		const { calls, apply } = fakeApply((m) => (isReplace(m) ? `${FAILED_PRECONDITION_TOKEN} the document changed` : null));
		const outcome = await saveBuffer(apply, "todo.txt", { text: "a\nb\n", hash: "old" }, "a\nb\nc\n");
		expect(outcome).toEqual({ how: "delta" });
		expect(calls).toHaveLength(2);
		expect(calls[1]).toEqual([{ kind: "add", line: "c" }]);
	});

	it("throws any other refusal and does not fall back", async () => {
		const { calls, apply } = fakeApply(() => "daemon rpc: code: 'Invalid argument'");
		await expect(saveBuffer(apply, "todo.txt", { text: "a\n", hash: "h" }, "b\n")).rejects.toBe(
			"daemon rpc: code: 'Invalid argument'"
		);
		expect(calls).toHaveLength(1);
	});

	it("uses the per-line delta alone when the baseline hash is not known", async () => {
		const { calls, apply } = fakeApply();
		expect(await saveBuffer(apply, "todo.txt", { text: "a\n", hash: "" }, "a\nb\n")).toEqual({ how: "delta" });
		expect(calls).toEqual([[{ kind: "add", line: "b" }]]);
	});

	it("tells a stale base from other errors by the bridge's token", () => {
		expect(isStaleBase(`${FAILED_PRECONDITION_TOKEN} x`)).toBe(true);
		expect(isStaleBase("daemon rpc: code: 'Unavailable'")).toBe(false);
	});
});
