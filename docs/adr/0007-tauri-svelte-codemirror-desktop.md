# 0007 — Desktop UI is Tauri 2 + Svelte 5 + CodeMirror 6

- Status: accepted
- Date: 2026-09-11
- Deciders: project owner (plan §1, decision 007; do not relitigate)

## Context
The UI model is the file (design §7): a syntax-highlighted document with real line numbers, popover edits, and a recursive detail view. That is an editor, not a form.

## Decision
We will build the desktop app as a Tauri 2 shell with a Svelte 5 UI whose main view is a read-only CodeMirror 6 document using a custom todo.txt language (Lezer grammar generated from `specs/todotxt.abnf`).

## Consequences
- Good: line numbers, wrapping, virtualisation, search and decorations come free; the interactive-text-file feel is native to CM6.
- Bad: a second stack (TypeScript) enters the repo at M7 and needs its own `stack.md` mapping and CI lane.
- Neutral / follow-ups: see the plan milestone that lands it.

## Alternatives considered
- Electron: heavier runtime; no Rust-side process for free.
- Native per OS (AppKit/WinUI/GTK): three UIs to keep in step.
