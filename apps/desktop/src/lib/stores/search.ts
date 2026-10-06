// The header search's text (task desktop-ui-revamp): one query shared by the Tasks buffer and the
// Universal page, so the search follows you between them. Settings keeps its own filter.
import { writable } from "svelte/store";

/** What is typed in the header search; "" when empty. */
export const searchQuery = writable("");

/** Enter / Shift+Enter in the header search: step to the next (1) or previous (-1) hit. `n`
 * counts presses, so the same direction twice is still a change. */
export const searchStep = writable<{ dir: 1 | -1; n: number }>({ dir: 1, n: 0 });

/** How many Universal rows match the header search (its count pill on that page). */
export const universalHits = writable(0);
