// Vitest: https://vitest.dev/api/
import { describe, expect, it } from "vitest";
import { lineOfTask, taskIdAt } from "../taskIds";

const A = "01M2T868JD32M84JQQ2ABASXW4";
const B = "01M2T868JD32M84JQQ2ABASXW5";

describe("taskIdAt", () => {
	it("takes the daemon's id for a Sidecar line that has no id: tag", () => {
		const contents = { text: "one\n\ntwo\n", task_ids: [A, "", B] };
		expect(taskIdAt(contents, 1)).toBe(A);
		expect(taskIdAt(contents, 3)).toBe(B);
	});

	it("answers no id for a blank line and for a line past the end", () => {
		const contents = { text: "one\n\n", task_ids: [A, ""] };
		expect(taskIdAt(contents, 2)).toBe("");
		expect(taskIdAt(contents, 9)).toBe("");
	});

	it("does not read a leftover id: word as the identity when the daemon sent ids", () => {
		const contents = { text: `one id:${B}\n\n`, task_ids: [A, ""] };
		expect(taskIdAt(contents, 1)).toBe(A);
		expect(taskIdAt({ text: `\nx id:${B}\n`, task_ids: ["", A] }, 1)).toBe("");
	});

	it("falls back to the id: tag when the daemon sent no ids (an older daemon)", () => {
		expect(taskIdAt({ text: `one id:${A}\ntwo\n` }, 1)).toBe(A);
		expect(taskIdAt({ text: `one id:${A}\ntwo\n`, task_ids: [] }, 1)).toBe(A);
		expect(taskIdAt({ text: `one id:${A}\ntwo\n` }, 2)).toBe("");
	});
});

describe("lineOfTask", () => {
	it("follows a task to its new line after it moved (a completed line goes to the bottom)", () => {
		const after = { text: "two\nthree\nx 2026-09-20 one\n", task_ids: [B, "01C", A] };
		expect(lineOfTask(after, A, 1)).toBe(3);
		expect(lineOfTask(after, B, 2)).toBe(1);
	});

	it("keeps the old line when there is nothing to follow by", () => {
		expect(lineOfTask({ text: "one\n" }, A, 1)).toBe(1); // an older daemon: no ids
		expect(lineOfTask({ text: "one\n", task_ids: [B] }, A, 1)).toBe(1); // the task is gone
		expect(lineOfTask({ text: "one\n\n", task_ids: [A, ""] }, "", 2)).toBe(2); // no id known yet
	});
});
