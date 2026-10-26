// Active-workspace root (ADR 0025, task desktop-workspace-switcher): shared between MainView
// (keys the file view so a workspace switch remounts FileView/ConflictBanner against the
// daemon's new selector — see WorkspaceSwitcher.svelte's module doc for why a remount is
// required) and WorkspaceSwitcher (reads/writes it around switchWorkspace calls). No Tauri
// import, same rationale as $lib/stores/duplicates.ts: this stays unit-testable without mocking
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

