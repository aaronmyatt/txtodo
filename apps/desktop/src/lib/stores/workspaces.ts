// Active-workspace root (ADR 0025, task desktop-workspace-switcher): shared between MainView
// (keys the file view so a workspace switch remounts FileView/ConflictBanner against the
// daemon's new selector — see WorkspaceSwitcher.svelte's module doc for why a remount is
// required) and WorkspaceSwitcher (reads/writes it around switchWorkspace calls). No Tauri
// import, same rationale as $lib/stores/conflicts.ts: this stays unit-testable without mocking
// invoke/listen, even though today it's too small a store to need its own test file.
import { writable } from "svelte/store";

export const currentWorkspaceRoot = writable<string>("");
