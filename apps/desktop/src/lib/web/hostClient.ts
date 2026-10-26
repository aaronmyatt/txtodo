// The page's side of the Worker host (`./host.worker.ts`): one Worker, calls matched to answers
// by id, change events fanned out to subscribers. Vite bundles the Worker from the `new URL(...)`
// form. Refs: https://vite.dev/guide/features.html#import-with-constructors ·
// https://developer.mozilla.org/en-US/docs/Web/API/Worker/postMessage

type Pending = { resolve: (value: unknown) => void; reject: (err: Error) => void };

/** Most calls waiting at once; past it a call is refused rather than queued without end. */
export const MAX_PENDING = 1024;

let worker: Worker | null = null;
let nextId = 1;
const pending = new Map<number, Pending>();
const changeListeners = new Set<(change: unknown) => void>();

function start(): Worker {
	if (worker) return worker;
	const w = new Worker(new URL("./host.worker.ts", import.meta.url), { type: "module" });
	w.onmessage = (e: MessageEvent<{ id?: number; ok?: unknown; err?: string; changes?: unknown[] }>) => {
		const { id, ok, err, changes } = e.data;
		if (changes) {
			for (const change of changes) for (const cb of changeListeners) cb(change);
			return;
		}
		const call = id === undefined ? undefined : pending.get(id);
		if (!call || id === undefined) return;
		pending.delete(id);
		if (err === undefined) call.resolve(ok);
		else call.reject(new Error(err));
	};
	w.onerror = (e) => {
		for (const call of pending.values()) call.reject(new Error(`web host: ${e.message}`));
		pending.clear();
	};
	worker = w;
	return w;
}

/** One host command; resolves with its JSON result, rejects with the host's message. */
export function callHost<T>(cmd: string, args: unknown): Promise<T> {
	if (pending.size >= MAX_PENDING) return Promise.reject(new Error("web host: too many calls waiting"));
	const id = nextId++;
	return new Promise<T>((resolve, reject) => {
		pending.set(id, { resolve: resolve as (value: unknown) => void, reject });
		start().postMessage({ id, cmd, args });
	});
}

/** Calls `cb` with each `ChangeDto` the host reports; returns the unsubscribe. */
export function onHostChange(cb: (change: unknown) => void): () => void {
	changeListeners.add(cb);
	return () => changeListeners.delete(cb);
}
