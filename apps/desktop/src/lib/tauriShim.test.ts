// tasks/logging-frontend (root todo.txt ref:logging-frontend): asserts `invoke`/`listen`'s
// behavior is unchanged for callers, and that the logging wrapper forwards the documented shape
// into the real `ui_log` Tauri command — never into `mockInvoke` (which has no "ui_log" case) and
// never leaking raw call arguments (task/note text) into the logged `fields`.
//
// `hasTauri()` (tauriShim.ts) is false in this plain-Node vitest environment (no `window`, and
// `import.meta.env.MODE` is not "e2e"), so `invoke()`/`listen()` dispatch through `mockInvoke`/
// `mockListen` exactly as they do under `npm run dev` — proving the mock backend keeps working
// unchanged by this task. `logToRust` always goes through the real `@tauri-apps/api/core` `invoke`
// though (by design — see notes.md), so that's the one we mock and spy on here.
// Ref: https://vitest.dev/api/vi.html#vi-mock
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { invoke as realTauriInvoke } from "@tauri-apps/api/core";

vi.mock("@tauri-apps/api/core", () => ({
	invoke: vi.fn()
}));

describe("tauriShim invoke/listen logging", () => {
	// Fresh module instance per test: tauriShim.ts's `requestIdCounter` is module-level state, and
	// mockInvoke's own module-level state (workspaces/files/etc, see ./mock/state.ts) is likewise
	// shared across imports — resetting both keeps tests independent of run order.
	beforeEach(() => {
		vi.resetModules();
		vi.mocked(realTauriInvoke).mockReset();
	});

	afterEach(() => {
		vi.restoreAllMocks();
	});

	it("invoke() still resolves with mockInvoke's real return value (mock backend unaffected)", async () => {
		const { invoke } = await import("./tauriShim");
		vi.mocked(realTauriInvoke).mockResolvedValue(undefined);
		const status = await invoke("daemon_status");
		expect(status).toBe("connected"); // ./mock/tauriMock.ts's daemon_status case
	});

	it("invoke() forwards ui_invoke_start then ui_invoke_ok with {command, ms, request_id}", async () => {
		const { invoke } = await import("./tauriShim");
		vi.mocked(realTauriInvoke).mockResolvedValue(undefined);
		await invoke("daemon_status");

		// Every call to the mocked `invoke` here is `logToRust`'s own forwarding to "ui_log" — the
		// real command dispatch went through `mockInvoke` instead (asserted above), never through
		// this mocked import.
		const calls = vi.mocked(realTauriInvoke).mock.calls;
		expect(calls.every(([cmd]) => cmd === "ui_log")).toBe(true);

		const startCall = calls.find(([, args]) => (args as { fields: { command: string } }).fields.command === "daemon_status" && (args as { message: string }).message === "ui_invoke_start");
		expect(startCall?.[1]).toMatchObject({ level: "info", message: "ui_invoke_start", fields: { command: "daemon_status" } });
		expect((startCall?.[1] as { fields: { request_id: string } }).fields.request_id).toMatch(/^r\d+$/);

		const okCall = calls.find(([, args]) => (args as { message: string }).message === "ui_invoke_ok");
		expect(okCall?.[1]).toMatchObject({ level: "info", message: "ui_invoke_ok", fields: { command: "daemon_status" } });
		const okFields = (okCall?.[1] as { fields: { ms: number; request_id: string } }).fields;
		expect(typeof okFields.ms).toBe("number");
		expect(okFields.request_id).toBe((startCall?.[1] as { fields: { request_id: string } }).fields.request_id);
	});

	it("a rejected invoke() still rejects with the original error, and fires ui_invoke_err (warn) with only {command, ms, request_id, error}", async () => {
		const { invoke } = await import("./tauriShim");
		vi.mocked(realTauriInvoke).mockResolvedValue(undefined);

		await expect(invoke("get_file", { path: "does-not-exist.txt" })).rejects.toThrow(/unknown path/);

		const calls = vi.mocked(realTauriInvoke).mock.calls;
		const errCall = calls.find(([, args]) => (args as { message: string }).message === "ui_invoke_err");
		expect(errCall?.[1]).toMatchObject({ level: "warn", message: "ui_invoke_err", fields: { command: "get_file" } });
		// Only the closed field set below — never a spread of the original call's `args`.
		const errFields = (errCall?.[1] as { fields: Record<string, unknown> }).fields;
		expect(Object.keys(errFields).sort()).toEqual(["command", "error", "ms", "request_id"]);
	});

	it("never forwards raw call arguments (e.g. real task/note text) into any logged fields", async () => {
		const { invoke } = await import("./tauriShim");
		vi.mocked(realTauriInvoke).mockResolvedValue(undefined);

		const realTaskText = "Reply to design feedback on the login screen";
		await invoke("apply", { path: "todo.txt", mutations: [{ kind: "add", line: realTaskText }] });

		const calls = vi.mocked(realTauriInvoke).mock.calls;
		for (const [, args] of calls) {
			expect(JSON.stringify(args)).not.toContain(realTaskText);
			// Every "ui_log" call's own fields object is a closed literal — never the raw `args`.
			const fields = (args as { fields: Record<string, unknown> }).fields;
			expect(fields).not.toHaveProperty("mutations");
			expect(fields).not.toHaveProperty("args");
		}
	});

	it("logToRust failures never throw or break the caller (ui_log rejecting is swallowed)", async () => {
		const { invoke } = await import("./tauriShim");
		vi.mocked(realTauriInvoke).mockRejectedValue(new Error("no Tauri runtime"));
		// Must resolve with the real mockInvoke value despite every logToRust call rejecting.
		await expect(invoke("daemon_status")).resolves.toBe("connected");
	});

	it("listen() forwards ui_event with {event, request_id} on each payload arrival, and still calls the caller's handler", async () => {
		const { listen } = await import("./tauriShim");
		const { emit } = await import("./mock/state");
		vi.mocked(realTauriInvoke).mockResolvedValue(undefined);

		const received: unknown[] = [];
		await listen("daemon-status", (evt) => {
			received.push(evt.payload);
		});
		emit("daemon-status", "connected");

		expect(received).toEqual(["connected"]);
		const eventCall = vi.mocked(realTauriInvoke).mock.calls.find(([, args]) => (args as { message: string }).message === "ui_event");
		expect(eventCall?.[1]).toMatchObject({ level: "info", message: "ui_event", fields: { event: "daemon-status" } });
		expect((eventCall?.[1] as { fields: { request_id: string } }).fields.request_id).toMatch(/^r\d+$/);
	});
});
