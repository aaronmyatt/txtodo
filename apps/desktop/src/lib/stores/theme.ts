// Theme preference (dark-mode support): the user's explicit choice — "light", "dark", or
// "system" (follow the OS) — persisted to localStorage, plus the concrete "light"/"dark" value
// actually applied. `+layout.svelte` writes `resolvedTheme` onto `<html data-theme>`; app.css
// keys its dark-palette overrides off that attribute.
// Pure/no Tauri imports on purpose, same rationale as $lib/stores/conflicts.ts: plain store logic
// stays unit-testable without mocking anything browser-only.
// Ref (matchMedia / prefers-color-scheme): https://developer.mozilla.org/en-US/docs/Web/API/Window/matchMedia
// Ref (custom stores): https://svelte.dev/docs/svelte/stores#Custom-stores
import { derived, writable, type Readable } from "svelte/store";
import { browser } from "$app/environment";

export type ThemePreference = "light" | "dark" | "system";
export type ResolvedTheme = "light" | "dark";

const STORAGE_KEY = "txtodo-theme";

function isThemePreference(value: string | null): value is ThemePreference {
	return value === "light" || value === "dark" || value === "system";
}

function readStoredPreference(): ThemePreference {
	if (!browser) return "system";
	const stored = localStorage.getItem(STORAGE_KEY);
	return isThemePreference(stored) ? stored : "system";
}

function systemPrefersDark(): boolean {
	return browser && window.matchMedia("(prefers-color-scheme: dark)").matches;
}

export const themePreference = writable<ThemePreference>(readStoredPreference());

export function setThemePreference(next: ThemePreference): void {
	if (browser) localStorage.setItem(STORAGE_KEY, next);
	themePreference.set(next);
}

/** The toggle button's single action: light -> dark -> system (follow the OS) -> light. */
export function cycleThemePreference(): void {
	themePreference.update((current) => {
		const next = current === "light" ? "dark" : current === "dark" ? "system" : "light";
		if (browser) localStorage.setItem(STORAGE_KEY, next);
		return next;
	});
}

// Bridges matchMedia's `change` event into a Svelte store so a "system" preference re-resolves
// live (no reload needed) when the OS-level scheme flips while the app is open.
function createSystemPrefersDarkStore(): Readable<boolean> {
	return {
		subscribe(run) {
			run(systemPrefersDark());
			if (!browser) return () => {};
			const mql = window.matchMedia("(prefers-color-scheme: dark)");
			const onChange = (e: MediaQueryListEvent) => run(e.matches);
			mql.addEventListener("change", onChange);
			return () => mql.removeEventListener("change", onChange);
		}
	};
}

const systemPrefersDarkStore = createSystemPrefersDarkStore();

/** The concrete theme actually applied: `"system"` resolves against the live OS preference. */
export const resolvedTheme: Readable<ResolvedTheme> = derived(
	[themePreference, systemPrefersDarkStore],
	([preference, systemDark]) => (preference === "system" ? (systemDark ? "dark" : "light") : preference)
);
