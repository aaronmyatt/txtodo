// The header search's text (task desktop-ui-revamp): one query shared by the Tasks buffer and the
// Universal page, so the search follows you between them. Settings keeps its own filter.
import { writable } from "svelte/store";

/** What is typed in the header search; "" when empty. */
export const searchQuery = writable("");
