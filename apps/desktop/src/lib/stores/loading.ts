// How many daemon calls are currently waiting out a workspace the daemon is still opening (task
// daemon-early-bind): `txtodod` binds its socket first and opens workspaces in the background, so
// a call for one that is not ready yet is answered `Unavailable: workspace loading`, and
// `loadingRetry.ts` retries it. MainView shows "Opening this workspace" while this is above zero,
// instead of the Dead banner a failed connect gets.
// Ref (custom stores): https://svelte.dev/docs/svelte/stores#Custom-stores
import { writable } from "svelte/store";

export const openingWorkspace = writable(0);
