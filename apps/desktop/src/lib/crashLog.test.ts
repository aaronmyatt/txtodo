import { describe, expect, it } from "vitest";
import { describeError, installCrashLog } from "./crashLog";

describe("describeError", () => {
	it("reads an Error, a string and anything else", () => {
		const e = describeError(new TypeError("bad"));
		expect([e.name, e.message]).toEqual(["TypeError", "bad"]);
		expect(e.stack).toContain("bad");
		expect(describeError("boom")).toEqual({ name: "string", message: "boom", stack: "" });
		expect(describeError(42)).toEqual({ name: "number", message: "42", stack: "" });
	});
});

describe("installCrashLog", () => {
	it("logs window errors and unhandled rejections, and stops after unlisten", () => {
		const target = new EventTarget();
		const seen: string[] = [];
		const off = installCrashLog(target, (message, fields) => seen.push(`${message}: ${fields.message}`));

		const err = Object.assign(new Event("error"), { error: new Error("render broke"), filename: "x.js", lineno: 3 });
		target.dispatchEvent(err);
		target.dispatchEvent(Object.assign(new Event("unhandledrejection"), { reason: "no daemon" }));
		off();
		target.dispatchEvent(Object.assign(new Event("unhandledrejection"), { reason: "after" }));

		expect(seen).toEqual(["uncaught error: render broke", "unhandled rejection: no daemon"]);
	});
});
