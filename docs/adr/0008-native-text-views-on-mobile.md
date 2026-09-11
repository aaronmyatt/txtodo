# 0008 — Mobile UIs paint with native text views over txtodo_core::tokenize

- Status: accepted
- Date: 2026-09-11
- Deciders: project owner (plan §1, decision 008; do not relitigate)

## Context
Token boundaries must be identical on every platform, and mobile editing must feel native (selection, keyboard, accessibility).

## Decision
We will use iOS SwiftUI over a `UITextView` with `NSTextStorage` highlighting, and Android Jetpack Compose `BasicTextField` with `AnnotatedString`, both colouring spans returned by `txtodo_core::tokenize` via uniffi. UIs only paint; the core owns the grammar.

## Consequences
- Good: one tokenizer everywhere; native text behaviour on each OS.
- Bad: two UI codebases; the FFI must ship `tokenize` before any mobile screen exists.
- Neutral / follow-ups: see the plan milestone that lands it.

## Alternatives considered
- A web view on mobile: non-native editing and accessibility.
- Re-implementing the grammar per platform: drift.
