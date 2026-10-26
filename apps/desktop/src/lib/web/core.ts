// Stand-in for `@tauri-apps/api/core` under `vite dev --mode web` / `vite build --mode web`
// (vite.config.ts's alias; ADR 0042): the list commands go to `txtodo-web` in a Worker, the rest
// get the browser's local answers (`./local.ts`). Same signature as Tauri's, so `$lib/tauriShim.ts`
// and every caller are unchanged. Ref: https://v2.tauri.app/reference/javascript/api/namespacecore/#invoke
import { callHost } from "./hostClient";
import { HOST_COMMANDS, answerLocally } from "./local";

export type InvokeArgs = Record<string, unknown>;
export type InvokeOptions = Record<string, unknown>;

/** Mirrors Tauri's `invoke`. */
export async function invoke<T>(cmd: string, args?: InvokeArgs, _options?: InvokeOptions): Promise<T> {
	if (HOST_COMMANDS.has(cmd)) return callHost<T>(cmd, args ?? {});
	return answerLocally<T>(cmd, args ?? {});
}

/** Tauri's `isTauri`: there is no Tauri runtime here. */
export function isTauri(): boolean {
	return false;
}
