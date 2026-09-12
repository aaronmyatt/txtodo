// PLACEHOLDER — `desktop-main-view` (a parallel task) owns the real Lezer grammar for todo.txt
// lines (specs/todotxt.abnf) and will overwrite this file wholesale on merge. Until then this is a
// no-op CM6 `Extension` so `EditPopover.svelte` (tasks/desktop-edit-popover) has something to
// import: it adds no highlighting/parsing, it just keeps the module resolvable.
// Ref: https://codemirror.net/docs/ref/#state.Extension
import type { Extension } from "@codemirror/state";

export const todotxtLanguage: Extension = [];
