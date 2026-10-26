// The browser build's local answers (ADR 0042): every command the app calls is either the Worker
// host's or answered here, so no screen hangs on an unhandled one; anything else is refused with a
// message. Ref: https://vitest.dev/api/
import { describe, expect, it } from "vitest";
import { HOST_COMMANDS, WEB_ROOT, answerLocally, isLocal } from "./local";

describe("web local answers", () => {
	it("routes the list commands to the host and nothing twice", () => {
		for (const cmd of ["list_files", "get_file", "apply", "history", "watch"]) expect(HOST_COMMANDS.has(cmd)).toBe(true);
		for (const cmd of HOST_COMMANDS) expect(isLocal(cmd)).toBe(false);
	});

	it("answers the startup calls the shell makes before any list", () => {
		expect(answerLocally("daemon_status")).toBe("connected");
		expect(answerLocally("workspace_root")).toBe(WEB_ROOT);
		const [ws] = answerLocally<{ root: string; load_state: string }[]>("list_workspaces");
		expect(ws).toMatchObject({ root: WEB_ROOT, load_state: "ready" });
		expect(answerLocally("workspace_layout")).toMatchObject({ todo_file: "todo.txt" });
		expect(answerLocally("check_updates")).toMatchObject({ newer: false });
	});

	it("refuses an unknown command with a message, not a hang", () => {
		expect(() => answerLocally("pair_offer")).toThrow(/not available in the browser/);
	});
});
