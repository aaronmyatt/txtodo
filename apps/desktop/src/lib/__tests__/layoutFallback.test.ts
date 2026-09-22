import { describe, expect, it } from "vitest";
import { layoutFallbackMessage } from "../layoutFallback";

describe("layoutFallbackMessage", () => {
	it("names the failure, the older-build cause and the fix", () => {
		const text = layoutFallbackMessage("Unimplemented: WorkspaceLayout");
		expect(text).toContain("(Unimplemented: WorkspaceLayout)");
		expect(text).toContain("older build");
		expect(text).toContain("txtodo daemon install");
	});

	it("omits the parenthesis when the error is blank", () => {
		expect(layoutFallbackMessage("  ")).not.toContain("(");
	});
});
