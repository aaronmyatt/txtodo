// Runtime switch between the real Tauri command bridge and the in-browser mock (`./mock/tauriMock`).
// `daemon.ts` and `devices/api.ts` import `invoke`/`listen` from here instead of `@tauri-apps/api`
// directly, so the same build renders in a real Tauri window (`npm run tauri dev`) and in a plain
// browser (`npm run dev`) — the latter for fast GUI iteration without a running txtodod.
// Detection ref: https://v2.tauri.app/reference/javascript/api/namespacecore/#isTauri (same check
// `isTauri()` performs internally).
import { invoke as tauriInvoke, type InvokeArgs, type InvokeOptions } from "@tauri-apps/api/core";
import { listen as tauriListen, type EventCallback, type EventName, type Options, type UnlistenFn } from "@tauri-apps/api/event";
import { mockInvoke, mockListen } from "./mock/tauriMock";

function hasTauri(): boolean {
	return typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;
}

export function invoke<T>(cmd: string, args?: InvokeArgs, options?: InvokeOptions): Promise<T> {
	return hasTauri() ? tauriInvoke<T>(cmd, args, options) : mockInvoke<T>(cmd, args as Record<string, unknown> | undefined);
}

export function listen<T>(event: EventName, handler: EventCallback<T>, options?: Options): Promise<UnlistenFn> {
	return hasTauri() ? tauriListen<T>(event, handler, options) : mockListen<T>(event, handler);
}

export type { UnlistenFn };
