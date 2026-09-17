// Window-stickiness (pin-on-top) preference (task desktop-always-on): the boolean itself is
// persisted to localStorage exactly like $lib/stores/theme.ts persists the theme preference (see
// that module's doc comment for the rationale — this app has no other settings-persistence
// mechanism to instead route through, so this is the same "for a per-viewer UI setting,
// localStorage is what already stands in for real persistence"). Unlike the theme preference,
// applying this one also has a real, live side effect (src-tauri/src/commands_window.rs::
// set_pinned calling `window.set_always_on_top`), so this store's setter always calls through to
// the Tauri command too — `MainView.svelte` also applies the stored value once at startup, since
// a freshly launched window always starts un-pinned regardless of what was stored last session.
//
// Relative imports only (`../tauriShim`, not `$lib/tauriShim`; no `$app/environment`), matching
// `daemon.ts`/`tauriShim.ts`'s own convention rather than `theme.ts`'s — this repo's minimal
// `vitest.config.ts` has no SvelteKit plugin, so a `$lib`/`$app` alias only resolves through the
// real SvelteKit build, not under `vitest`; a file that wants a `pin.test.ts` (this one does, see
// that file) needs plain relative imports and a `typeof window` check instead of `$app/
// environment`'s `browser`, the same reasoning `tauriShim.ts::hasTauri` already documents.
// Ref (custom stores): https://svelte.dev/docs/svelte/stores#Custom-stores
import { writable } from "svelte/store";
import { invoke } from "../tauriShim";

const STORAGE_KEY = "txtodo-pinned";

function hasLocalStorage(): boolean {
	return typeof localStorage !== "undefined";
}

export function readStoredPreference(): boolean {
	if (!hasLocalStorage()) return false;
	return localStorage.getItem(STORAGE_KEY) === "true";
}

export const pinned = writable<boolean>(readStoredPreference());

/** Applies `next` to the real window (best-effort: a failed IPC call is logged, never thrown -
 * matching `retryConnect`/`ui_log`'s own "never let a background call break the caller" style)
 * and persists it. */
export async function setPinned(next: boolean): Promise<void> {
	if (hasLocalStorage()) localStorage.setItem(STORAGE_KEY, String(next));
	pinned.set(next);
	try {
		await invoke("set_pinned", { pinned: next });
	} catch (e) {
		console.error("set_pinned failed", e);
	}
}

/** Re-applies the stored preference to a freshly created window; called once from
 * `MainView.svelte` on mount, since a new OS-level window always starts un-pinned. */
export function applyStoredPin(): Promise<void> {
	return setPinned(readStoredPreference());
}
