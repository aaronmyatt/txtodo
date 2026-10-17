// Stand-in for `@tauri-apps/api/event` in the web build (vite.config.ts's alias; ADR 0042). A
// `daemon-change` is each `ChangeDto` the Worker host reports after a call; `daemon-status` is
// "connected" at once (the host is in this tab). Other events (the tray's quick add, the global
// prompt-bar shortcut) never fire in a browser.
// Ref: https://v2.tauri.app/reference/javascript/api/namespaceevent/#listen
import { onHostChange } from "./hostClient";

export type UnlistenFn = () => void;
export type EventName = string;
export type Options = Record<string, unknown>;
export interface Event<T> {
	event: EventName;
	id: number;
	payload: T;
}
export type EventCallback<T> = (event: Event<T>) => void;

let nextEventId = 1;

/** Mirrors Tauri's `listen`. */
export async function listen<T>(event: EventName, handler: EventCallback<T>, _options?: Options): Promise<UnlistenFn> {
	if (event === "daemon-status") {
		handler({ event, id: nextEventId++, payload: "connected" as T });
		return () => {};
	}
	if (event === "daemon-change") {
		return onHostChange((change) => handler({ event, id: nextEventId++, payload: change as T }));
	}
	return () => {};
}
