// TODO(placeholder): the real todo.txt Lezer grammar + CM6 language package (task
// `desktop-lezer-grammar`) is being built on a parallel branch and will overwrite this file on
// merge. It is expected to export `todotxtLanguage` as a CM6 `LanguageSupport`/`Extension` built
// from `specs/todotxt.abnf`, mapping plan §3.1's semantic token names (`priority`, `date`,
// `completion-marker`, `project`, `context`, `tag-key`, `tag-value`, `id-tag`, `text`) to colours
// via `styleTags`/a CM6 theme. Until then this is a no-op so `FileView.svelte`/`EditPopover.svelte`
// compile and render (with no syntax colouring) rather than being blocked on the other branch.
//
// CM6 language docs: https://codemirror.net/docs/ref/#language
// CM6 extension docs: https://codemirror.net/docs/ref/#state.Extension
import { EditorView } from "@codemirror/view";
import type { Extension } from "@codemirror/state";

/** No-op placeholder: contributes no tokens, no highlighting, just an empty theme extension. */
export const todotxtLanguage: Extension = EditorView.theme({});
