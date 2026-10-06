// Uncaught frontend errors into desktop.log (task desktop-ui-revamp): a render crash or a
// rejected promise nobody awaited used to vanish (the Universal page hung on "Loading…" with
// nothing logged, 2026-09-24). The window's `error` and `unhandledrejection` events and the
// layout's <svelte:boundary> all end up here, then in `ui_log` at error level.
// https://developer.mozilla.org/en-US/docs/Web/API/Window/error_event
// https://developer.mozilla.org/en-US/docs/Web/API/Window/unhandledrejection_event

/** Longest stack kept: enough to place the error, not a wall of frames. */
const STACK_MAX = 4000;

export interface ErrorFields {
	name: string;
	message: string;
	stack: string;
}

/** Any thrown value as plain fields (a string, an Error, or something else entirely). */
export function describeError(e: unknown): ErrorFields {
	if (e instanceof Error) {
		return { name: e.name, message: e.message, stack: (e.stack ?? "").slice(0, STACK_MAX) };
	}
	return { name: typeof e, message: typeof e === "string" ? e : String(e), stack: "" };
}

type Log = (message: string, fields: Record<string, unknown>) => void;

/** Listens on `target` (the window) and logs each uncaught error once; returns the unlisten. */
export function installCrashLog(target: EventTarget, log: Log): () => void {
	const onError = (ev: Event) => {
		const e = ev as ErrorEvent;
		log("uncaught error", { ...describeError(e.error ?? e.message), source: e.filename ?? "", line: e.lineno ?? 0 });
	};
	const onRejection = (ev: Event) => {
		log("unhandled rejection", { ...describeError((ev as PromiseRejectionEvent).reason) });
	};
	target.addEventListener("error", onError);
	target.addEventListener("unhandledrejection", onRejection);
	return () => {
		target.removeEventListener("error", onError);
		target.removeEventListener("unhandledrejection", onRejection);
	};
}
