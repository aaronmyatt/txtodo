import { describe, expect, it } from "vitest";
import { imageLink, imageMarkdown } from "./noteImages";

describe("note images", () => {
	it("finds an assets/ image link on a line", () => {
		expect(imageLink("see ![map](assets/1a2b3c4d-map.png) here")).toEqual({ alt: "map", link: "assets/1a2b3c4d-map.png" });
		expect(imageLink("![x](https://example.com/x.png)")).toBeNull();
		expect(imageLink("plain text")).toBeNull();
	});

	it("turns a dropped file into one markdown line", () => {
		expect(imageMarkdown("My [map].png", "assets/1a2b3c4d-My-map-.png")).toBe("![My map](assets/1a2b3c4d-My-map-.png)");
	});
});
