// Runtime switch between the real Tauri command bridge and the in-browser mock (`./mock/tauriMock`).
// `daemon.ts` and `devices/api.ts` import `invoke`/`listen` from here instead of `@tauri-apps/api`
// directly, so the same build renders in a real Tauri window (`npm run tauri dev`) and in a plain
// browser (`npm run dev`) — the latter for fast GUI iteration without a running txtodod.
// Detection ref: https://v2.tauri.app/reference/javascript/api/namespacecore/#isTauri (same check
// `isTauri()` performs internally).
import { invoke as tauriInvoke, type InvokeArgs, type InvokeOptions } from "@tauri-apps/api/core";
import { listen as tauriListen, type EventCallback, type EventName, type Options, type UnlistenFn } from "@tauri-apps/api/event";
import { mockInvoke, mockListen } from "./mock/tauriMock";

// BUGFIX (found while running tasks/desktop-visual-regression's Playwright suite): a plain
// Chromium browser (no Tauri runtime) never has `window.__TAURI_INTERNALS__`, so `hasTauri()`
// used to return `false` unconditionally under `vite dev --mode e2e` too — meaning EVERY call
// silently fell through to `mockInvoke`/`mockListen` (in-browser demo data, see ./mock/state.ts's
// "Ship the auth rewrite" fixture) instead of the real `@tauri-apps/api/core`/`event` imports that
// `vite.config.ts`'s `mode === "e2e"` alias points at `../e2e/shim/{core,event}.ts`. That shim is
// the entire point of tasks/desktop-playwright-tests' harness ("the code under test is the real
// reconciler, real file bytes, real op log" — its notes.md) — with this gate wrong, the harness
// was mocked the whole time, for every spec, not just this task's new ones (confirmed directly:
// a Playwright page under this bug rendered the mock's "Ship the auth rewrite"/"release-notes2"
// demo tasks instead of any seeded fixture). `import.meta.env.MODE` is Vite's own build-time mode
// string (https://vite.dev/guide/env-and-mode.html#modes) — "e2e" only when `--mode e2e` is
// passed, so this changes nothing for the real Tauri app or the plain `npm run dev` preview.
function hasTauri(): boolean {
	return (typeof window !== "undefined" && "__TAURI_INTERNALS__" in window) || import.meta.env.MODE === "e2e";
}

export function invoke<T>(cmd: string, args?: InvokeArgs, options?: InvokeOptions): Promise<T> {
	return hasTauri() ? tauriInvoke<T>(cmd, args, options) : mockInvoke<T>(cmd, args as Record<string, unknown> | undefined);
}

export function listen<T>(event: EventName, handler: EventCallback<T>, options?: Options): Promise<UnlistenFn> {
	return hasTauri() ? tauriListen<T>(event, handler, options) : mockListen<T>(event, handler);
}

export type { UnlistenFn };
