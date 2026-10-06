// Vitest: https://vitest.dev/api/
import { describe, expect, it } from "vitest";
import { addOnTopMutations } from "../addOnTop";

const A = "01M2T868JD32M84JQQ2ABASXW4";

describe("addOnTopMutations", () => {
	it("adds, then moves the new line above the first task", () => {
		const text = "\nbuy milk\n\ncall mom\n\n";
		expect(addOnTopMutations(text, ["", A, "", "", ""], "walk dog")).toEqual([
			{ kind: "add", line: "walk dog" },
			{
				kind: "move_before",
				task: { line_number: 5, task_id: "" },
				before: { line_number: 2, task_id: A }
			}
		]);
	});

	it("only adds to an empty list", () => {
		expect(addOnTopMutations("", [], "walk dog")).toEqual([{ kind: "add", line: "walk dog" }]);
		expect(addOnTopMutations("\n\n", ["", ""], "walk dog")).toEqual([{ kind: "add", line: "walk dog" }]);
	});
});
