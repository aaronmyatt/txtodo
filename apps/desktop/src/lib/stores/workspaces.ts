// Active-workspace root (ADR 0025, task desktop-workspace-switcher): shared between MainView
// (keys the file view so a workspace switch remounts FileView/ConflictBanner against the
// daemon's new selector — see WorkspaceSwitcher.svelte's module doc for why a remount is
// required) and WorkspaceSwitcher (reads/writes it around switchWorkspace calls). No Tauri
// import, same rationale as $lib/stores/conflicts.ts: this stays unit-testable without mocking
// invoke/listen, even though today it's too small a store to need its own test file.
import { writable } from "svelte/store";
import { DEFAULT_LAYOUT, type RefLayout } from "$lib/todotxt/lineInfo";

export const currentWorkspaceRoot = writable<string>("");

/** The current workspace's layout, refetched whenever the workspace changes (MainView). Starts on
 * the daemon's default so the first paint composes `tasks/<slug>` before the answer arrives. */
export const workspaceLayoutStore = writable<RefLayout>(DEFAULT_LAYOUT);

/** The last `workspace_layout` failure for the current workspace, `""` when it answered (task
 * desktop-notes-hidden): MainView shows it instead of silently assuming `tasks/`. */
export const workspaceLayoutError = writable<string>("");

/** One detail level the universal view (ADR 0025, task desktop-universal-view) asked MainView to
 * open after switching workspaces — consumed exactly once by MainView's own
 * `$currentWorkspaceRoot` effect, which is what actually pushes it onto the detail stack (see
 * that effect's doc comment for why the consumption has to live there, not here). `null` means
 * nothing is pending; a plain value, not a queue, since only one switch is ever in flight. */
export interface PendingUniversalNav {
	file: string;
	line: number;
	workspaceRoot: string;
}

export const pendingUniversalNav = writable<PendingUniversalNav | null>(null);
