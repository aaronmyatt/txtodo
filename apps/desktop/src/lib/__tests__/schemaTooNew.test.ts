// Vitest: https://vitest.dev/api/
import { describe, expect, it } from "vitest";
import { friendlySchemaError } from "../schemaTooNew";

describe("friendlySchemaError", () => {
	it("rewrites the daemon's raw refusal and names both format numbers", () => {
		const raw =
			"daemon rpc: code: 'Internal error', message: \"open /Users/oya/Development/txtodo: store: database schema version 8 is newer than supported 7\"";
		const message = friendlySchemaError(raw);
		expect(message).toContain("upgraded by a newer txtodo");
		expect(message).toContain("data format 8");
		expect(message).toContain("up to 7");
		expect(message).toContain("Update the app");
		expect(message).not.toContain("daemon rpc");
	});

	it("also reads an Error, not only a string", () => {
		expect(friendlySchemaError(new Error("database schema version 9 is newer than supported 8"))).toContain(
			"data format 9"
		);
	});

	it("leaves every other error alone", () => {
		expect(friendlySchemaError("daemon rpc: code: 'Unavailable', message: \"workspace loading\"")).toBeNull();
		expect(friendlySchemaError("database schema version 7 is older than supported 8")).toBeNull();
	});
});
