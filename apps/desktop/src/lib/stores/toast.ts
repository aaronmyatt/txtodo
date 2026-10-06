// App-wide toasts (task desktop-ui-revamp): a short line at the bottom right, optionally with an
// Undo, gone after TOAST_MS. `Toasts.svelte` in the shell renders them.
import { writable } from "svelte/store";

export const TOAST_MS = 5000;

export interface Toast {
	id: number;
	message: string;
	undo?: () => void;
}

export const toasts = writable<Toast[]>([]);

let nextId = 1;

export function dismissToast(id: number): void {
	toasts.update((list) => list.filter((t) => t.id !== id));
}

/** Shows `message` for TOAST_MS; `undo` adds an Undo button that runs it once and dismisses. */
export function showToast(message: string, undo?: () => void): number {
	const id = nextId++;
	toasts.update((list) => [...list, { id, message, undo }]);
	setTimeout(() => dismissToast(id), TOAST_MS);
	return id;
}
