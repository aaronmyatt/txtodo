// List mode's `m` and `c` (specs/client-parity.toml list.move_to, list.copy_to; task
// add-the-ability-to-move-or-copy-a-task-t): FileView opens the line's detail, and the detail
// focuses its Move to… or Copy to… picker once it has the lists to offer. One shot: the detail
// clears it as it takes it.
// Svelte stores: https://svelte.dev/docs/svelte/stores
import { writable } from "svelte/store";

export type PickerIntent = "" | "move" | "copy";

export const movePicker = writable<PickerIntent>("");
