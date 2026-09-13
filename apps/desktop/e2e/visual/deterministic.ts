// Shared setup for every visual-regression spec under this directory
// (tasks/desktop-visual-regression/notes.md: "disable animations/transitions, freeze the clock...
// otherwise goldens drift on every run").
//
// Animations/transitions: `expect(page).toHaveScreenshot()` already defaults to
// `animations: "disabled"` (https://playwright.dev/docs/api/class-pageassertions#page-assertions-to-have-screenshot-1),
// which freezes CSS animations/transitions/Web Animations at their initial state — including
// CodeMirror's own `cm-blink` cursor-blink keyframe (`@codemirror/view`'s `drawSelection`
// extension), so the popover's blinking caret does not need any extra handling here. This module
// exists for the one thing Playwright's default does NOT cover: freezing the clock, for any
// surface that renders a relative-time string (`$lib/components/editPopoverLogic.ts::formatRelativeTime`,
// e.g. the edit popover's "Line N · device, 3m ago" footer).
//
// None of the fixtures this task's specs use ever produce that footer text in the first place —
// they seed `todo.txt` directly on disk rather than through the daemon's op log
// (`e2e/fixtures.ts::seed`), so `history()` finds zero ops for the task and the footer falls back
// to the plain "Line N" (`EditPopover.svelte`'s `footer` derivation). Exporting `FROZEN_NOW` here
// regardless, and wiring it in at the one call site (`Date.now` inside `formatRelativeTime`) if a
// future golden ever needs a real history entry — see that function's signature, which already
// takes `now` as an explicit, overridable parameter for exactly this reason.
export const FROZEN_NOW_MS = Date.parse("2026-01-15T12:00:00.000Z");
