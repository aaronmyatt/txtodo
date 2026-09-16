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

// tasks/logging-frontend (root todo.txt ref:logging-frontend): frontend-only correlation id, minted
// once per `invoke()` call, so a human reading the forwarded log can line up one call's own
// ui_invoke_start/ok/err triplet. It is never sent on the wire to any real Tauri command and has no
// relationship to the Rust-side span id — that correlation stays timestamp+command-name based (see
// tasks/logging-frontend/notes.md "Local request_id" for why: threading one real shared id would be
// a breaking signature change across all 26 command fns, out of scope here).
let requestIdCounter = 0;
function nextRequestId(): string {
	requestIdCounter += 1;
	return `r${requestIdCounter}`;
}

// Forwards one structured line into the same Rust `tracing` timeline `logging-desktop` built
// (`apps/desktop/src-tauri/src/commands.rs::ui_log`). Always goes through the *real*
// `@tauri-apps/api/core` invoke, never this module's own `invoke` below or `mockInvoke` — logging
// must not recurse through the wrapper it instruments, and `mockInvoke` has no "ui_log" case (it
// throws "unhandled command" for anything it doesn't recognize). Swallows every failure: under
// `npm run dev` (no Tauri runtime) this call always rejects, and per this task's brief, `ui_log`
// failing must never itself throw or break the caller's own business logic.
// Ref (Tauri v2 `invoke`): https://v2.tauri.app/reference/javascript/api/namespacecore/#invoke
function logToRust(level: "info" | "warn", message: string, fields: Record<string, unknown>): void {
	tauriInvoke<void>("ui_log", { level, message, fields }).catch(() => {
		// Deliberately silent — see doc comment above.
	});
}

export function invoke<T>(cmd: string, args?: InvokeArgs, options?: InvokeOptions): Promise<T> {
	const requestId = nextRequestId();
	// `performance.now()`: monotonic, sub-ms, matches how the Rust-side `tracing` spans measure
	// elapsed time — avoids wall-clock (`Date.now()`) skew from clock adjustments.
	// Ref: https://developer.mozilla.org/en-US/docs/Web/API/Performance/now
	const startedAt = performance.now();
	logToRust("info", "ui_invoke_start", { command: cmd, request_id: requestId });
	const result = hasTauri() ? tauriInvoke<T>(cmd, args, options) : mockInvoke<T>(cmd, args as Record<string, unknown> | undefined);
	return result.then(
		(value) => {
			const ms = performance.now() - startedAt;
			logToRust("info", "ui_invoke_ok", { command: cmd, ms, request_id: requestId });
			return value;
		},
		(err) => {
			const ms = performance.now() - startedAt;
			// `error` is the stringified failure only — never the original `args`, which may carry
			// real task/note text (e.g. `apply`'s `mutations`, `edit_notes`'s `newText`).
			logToRust("warn", "ui_invoke_err", { command: cmd, ms, request_id: requestId, error: String(err) });
			throw err;
		}
	);
}

export function listen<T>(event: EventName, handler: EventCallback<T>, options?: Options): Promise<UnlistenFn> {
	const wrapped: EventCallback<T> = (evt) => {
		// A fresh id per *arrival*, not per subscription — each event delivery is its own loggable
		// occurrence, distinct from an `invoke()` call's start/ok/err triplet above.
		logToRust("info", "ui_event", { event, request_id: nextRequestId() });
		return handler(evt);
	};
	return hasTauri() ? tauriListen<T>(event, wrapped, options) : mockListen<T>(event, wrapped);
}

export type { UnlistenFn };
