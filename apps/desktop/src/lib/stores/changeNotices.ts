// Settings > General's "Notify on changes" toggle (task change-notifications): OS notifications
// when another client (an agent, the CLI, another device, an editor save) edits, completes or
// deletes tasks. Kept in localStorage like `pin.ts`, and mirrored to the Rust side
// (`notify.rs::set_change_notices`), which decides whether a notice is shown. On by default.
// Relative imports only, for vitest (see `pin.ts`'s doc).
// Ref (custom stores): https://svelte.dev/docs/svelte/stores#Custom-stores
import { writable } from "svelte/store";
import { invoke } from "../tauriShim";

const STORAGE_KEY = "txtodo-change-notices";

export function readStoredPreference(): boolean {
	if (typeof localStorage === "undefined") return true;
	return localStorage.getItem(STORAGE_KEY) !== "false";
}

export const changeNotices = writable<boolean>(readStoredPreference());

/** Stores `next` and tells the Rust side; a failed call is logged, never thrown. */
export async function setChangeNotices(next: boolean): Promise<void> {
	if (typeof localStorage !== "undefined") localStorage.setItem(STORAGE_KEY, String(next));
	changeNotices.set(next);
	try {
		await invoke("set_change_notices", { enabled: next });
	} catch (e) {
		console.error("set_change_notices failed", e);
	}
}

/** Re-applies the stored preference at start: the Rust side always starts with notices on. */
export function applyStoredChangeNotices(): Promise<void> {
	return setChangeNotices(readStoredPreference());
}
