// todotxtLanguage.ts — the CM6 language pack seam for todo.txt (plan M7, §3.1).
//
// Wraps the generated Lezer parser (apps/desktop/src/lang/todotxt.parser.js, produced from
// specs/todotxt.abnf by apps/desktop/scripts/abnf-to-lezer.mjs — see that file for how the grammar
// is derived) as a CodeMirror 6 language:
//   - LRLanguage.define(): https://codemirror.net/docs/ref/#language.LRLanguage^define
//   - styleTags(): https://lezer.codemirror.net/docs/ref/#highlight.styleTags
//   - HighlightStyle / syntaxHighlighting(): https://codemirror.net/docs/ref/#language.HighlightStyle
//
// This is the exact seam desktop-main-view and desktop-edit-popover import (they run in separate
// worktrees in parallel with this task, and are already told to expect this path/name): the
// popover's single-line editor and the main view's read-only document view share this one
// grammar, so highlighting never drifts between the two.
//
// Each of the plan §3.1 semantic token names (completion-marker, priority, date, project, context,
// tag-key, tag-value, id-tag, text) gets its own `@lezer/highlight` Tag, distinct from the generic
// tags in `@lezer/highlight`'s default set — todo.txt's categories aren't "a string" or "a keyword",
// they're domain-specific, so a custom tag per name is more honest than reaching for a
// similarly-shaped generic one. `todotxtHighlightStyle` maps each to a `tok-<name>` CSS class so a
// theme can color them later (colors are deliberately not picked here — that's a UI-task decision).
import { parser } from "../../lang/todotxt.parser.js";
import { LRLanguage, LanguageSupport, HighlightStyle, syntaxHighlighting } from "@codemirror/language";
import { styleTags, Tag } from "@lezer/highlight";

// Grammar node name -> plan §3.1 token name. Node names are PascalCase because Lezer identifiers
// can't contain "-"; this is the only place that PascalCase-to-kebab-case mapping lives.
export const TOKEN_NAME_BY_NODE = {
  CompletionMarker: "completion-marker",
  Priority: "priority",
  Date: "date",
  Project: "project",
  Context: "context",
  TagKey: "tag-key",
  TagValue: "tag-value",
  IdTag: "id-tag",
  Text: "text",
} as const;

export type TodotxtTokenName = (typeof TOKEN_NAME_BY_NODE)[keyof typeof TOKEN_NAME_BY_NODE];

// One custom highlight.Tag per §3.1 name, keyed by that same kebab-case name.
export const todotxtTags: Record<TodotxtTokenName, Tag> = Object.fromEntries(
  Object.values(TOKEN_NAME_BY_NODE).map((name) => [name, Tag.define(name)]),
) as Record<TodotxtTokenName, Tag>;

const parserWithHighlight = parser.configure({
  props: [
    styleTags(
      Object.fromEntries(
        Object.entries(TOKEN_NAME_BY_NODE).map(([node, name]) => [node, todotxtTags[name]]),
      ),
    ),
  ],
});

export const todotxtLanguageObj = LRLanguage.define({
  name: "todotxt",
  parser: parserWithHighlight,
});

// A neutral default: one CSS class per token name (`tok-completion-marker`, `tok-priority`, ...),
// no colors. A real theme replaces this with its own HighlightStyle over the same `todotxtTags` (or
// just styles the `tok-*` classes directly); this is here so the language is usable/addressable out
// of the box without every consumer having to invent the node-name-to-class wiring themselves.
export const todotxtHighlightStyle = HighlightStyle.define(
  Object.entries(todotxtTags).map(([name, tag]) => ({ tag, class: `tok-${name}` })),
);

// The single export desktop-main-view / desktop-edit-popover import. A LanguageSupport is
// structurally an Extension (`{extension: Extension}` — https://codemirror.net/docs/ref/#state.Extension),
// so `extensions: [todotxtLanguage]` in an EditorState works directly.
export const todotxtLanguage = new LanguageSupport(todotxtLanguageObj, [
  syntaxHighlighting(todotxtHighlightStyle),
]);
