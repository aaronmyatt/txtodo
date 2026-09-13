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
// similarly-shaped generic one. `todotxtHighlightStyle` maps each to a `tok-<name>` CSS class (kept
// as a stable, overridable hook — a consumer can replace `todotxtColorTheme` with its own theme
// over the same classes); `todotxtColorTheme` supplies this file's own default colors for them.
// (`HighlightStyle.define` ignores any style properties given alongside an explicit `class` on the
// same spec entry — https://github.com/codemirror/language/blob/main/src/highlight.ts — so the
// colors have to live in a separate theme keyed to the class names, not inline in the spec.)
import { parser } from "../../lang/todotxt.parser.js";
import { LRLanguage, LanguageSupport, HighlightStyle, syntaxHighlighting } from "@codemirror/language";
import { EditorView } from "@codemirror/view";
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
// colors supplied separately by `todotxtColorTheme` below.
export const todotxtHighlightStyle = HighlightStyle.define(
  Object.entries(todotxtTags).map(([name, tag]) => ({ tag, class: `tok-${name}` })),
);

// This file's own default palette. Every color sits at the same Tailwind 500/700 lightness band so
// no one category reads "louder" than another by weight alone — only hue tells them apart. `text`
// (the plain description) is the one deliberately near-black entry: everything else is markup
// *about* the line, so it earns a color; the words a human actually wrote don't compete with them.
export const todotxtColorTheme = EditorView.baseTheme({
  ".tok-completion-marker": { color: "#15803d", fontWeight: "600" },
  ".tok-priority": { color: "#b45309", fontWeight: "700" },
  ".tok-date": { color: "#6d28d9" },
  ".tok-project": { color: "#1d4ed8" },
  ".tok-context": { color: "#be185d" },
  ".tok-tag-key": { color: "#57534e", fontWeight: "600" },
  ".tok-tag-value": { color: "#78716c" },
  ".tok-id-tag": { color: "#9ca3af" },
  ".tok-text": { color: "#111827" },
});

// The single export desktop-main-view / desktop-edit-popover import. A LanguageSupport is
// structurally an Extension (`{extension: Extension}` — https://codemirror.net/docs/ref/#state.Extension),
// so `extensions: [todotxtLanguage]` in an EditorState works directly.
export const todotxtLanguage = new LanguageSupport(todotxtLanguageObj, [
  syntaxHighlighting(todotxtHighlightStyle),
  todotxtColorTheme,
]);
