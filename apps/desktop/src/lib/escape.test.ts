import { describe, expect, it } from "vitest";
import { leaveEditor, onEditorEscape } from "./escape";

describe("leaveEditor", () => {
	it("blurs the editor and tells listeners until they unsubscribe", () => {
		let blurred = 0;
		let heard = 0;
		const el = { blur: () => blurred++ } as unknown as HTMLElement;
		const off = onEditorEscape(() => heard++);
		leaveEditor(el);
		off();
		leaveEditor(el);
		expect([blurred, heard]).toEqual([2, 1]);
	});
});
