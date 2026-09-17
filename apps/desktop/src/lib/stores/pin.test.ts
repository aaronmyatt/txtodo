// Unit test for the pin-on-top preference's persistence round trip (task desktop-always-on, item
// 6's automatable half: "pin toggle round-trips through a restart"). The other half of that item
// — a human confirming the window actually stays above others, a real OS-level effect no
// automated harness in this repo can assert — is left `@human`-flagged in
// tasks/desktop-always-on/notes.md, not simulated here.
//
// Runs in this repo's plain-Node vitest environment (no jsdom, no `localStorage` global enabled
// by default — Node's own is behind an experimental flag this repo doesn't set, confirmed by
// running this file against the bare global first). A minimal in-memory `Storage` stand-in is
// installed as `globalThis.localStorage` below rather than adding a jsdom/happy-dom
// devDependency just for this one file — `pin.ts`'s own `hasLocalStorage()` check
// (`typeof localStorage !== "undefined"`) sees this the same way it would see a real browser's.
//
// `../tauriShim`'s `invoke()` routes to `../mock/tauriMock`'s `mockInvoke` in this environment
// (no `window.__TAURI_INTERNALS__`, not `--mode e2e`) — the same path a plain `npm run dev`
// preview takes — so that module is what's mocked/spied on below, not `@tauri-apps/api/core`
// directly (that real import only ever carries this file's own `ui_log` forwarding, a separate,
// already-swallowed side channel — see `tauriShim.test.ts` for that specific behavior).
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { get } from "svelte/store";

vi.mock("../mock/tauriMock", async (importOriginal) => {
	const actual = await importOriginal<typeof import("../mock/tauriMock")>();
	return { ...actual, mockInvoke: vi.fn(actual.mockInvoke) };
});

const STORAGE_KEY = "txtodo-pinned";

function installFakeLocalStorage(): void {
	const data = new Map<string, string>();
	(globalThis as { localStorage?: Storage }).localStorage = {
		getItem: (key: string) => data.get(key) ?? null,
		setItem: (key: string, value: string) => {
			data.set(key, value);
		},
		removeItem: (key: string) => {
			data.delete(key);
		},
		clear: () => data.clear(),
		key: (index: number) => Array.from(data.keys())[index] ?? null,
		get length() {
			return data.size;
		}
	} satisfies Storage;
}

describe("pin-on-top preference", () => {
	beforeEach(() => {
		vi.resetModules();
		installFakeLocalStorage();
		localStorage.clear();
	});

	afterEach(() => {
		vi.restoreAllMocks();
		delete (globalThis as { localStorage?: Storage }).localStorage;
	});

	it("defaults to unpinned with nothing stored", async () => {
		const { pinned, readStoredPreference } = await import("./pin");
		expect(readStoredPreference()).toBe(false);
		expect(get(pinned)).toBe(false);
	});

	it("setPinned persists to localStorage and calls the set_pinned command", async () => {
		const { mockInvoke } = await import("../mock/tauriMock");
		const { setPinned, pinned } = await import("./pin");
		await setPinned(true);
		expect(get(pinned)).toBe(true);
		expect(localStorage.getItem(STORAGE_KEY)).toBe("true");
		expect(mockInvoke).toHaveBeenCalledWith("set_pinned", { pinned: true });
	});

	it("round-trips through a simulated restart: a fresh module import re-reads the stored value", async () => {
		const first = await import("./pin");
		await first.setPinned(true);

		// Simulate a restart: fresh module state (a new `pinned` store, re-initialized from
		// storage), but the same underlying localStorage a real OS-level relaunch would keep.
		vi.resetModules();
		const second = await import("./pin");
		expect(second.readStoredPreference()).toBe(true);
		expect(get(second.pinned)).toBe(true);
	});

	it("applyStoredPin re-applies whatever was last stored, e.g. for a freshly created window", async () => {
		const first = await import("./pin");
		await first.setPinned(true);

		vi.resetModules();
		const { mockInvoke } = await import("../mock/tauriMock");
		const second = await import("./pin");
		await second.applyStoredPin();
		expect(mockInvoke).toHaveBeenCalledWith("set_pinned", { pinned: true });
	});

	it("a failed set_pinned call never throws (best-effort, matches retryConnect/ui_log style)", async () => {
		const { mockInvoke } = await import("../mock/tauriMock");
		vi.mocked(mockInvoke).mockRejectedValueOnce(new Error("no window"));
		const { setPinned, pinned } = await import("./pin");
		await expect(setPinned(true)).resolves.toBeUndefined();
		expect(get(pinned)).toBe(true); // the store still updates; only the IPC call failed
	});
});
