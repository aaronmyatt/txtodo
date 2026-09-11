# Playwright tests for popover, detail, notes creation, nested breadcrumb, conflict sheet (plan M7, plan §7)

Plan M7 acceptance: "Playwright/WebDriver tests: click opens popover with the raw line including
`id:`; Enter saves and the file on disk changes only that line; double-click opens detail; typing
into empty notes creates the directory; sub-list line double-click nests the breadcrumb; conflict
sheet appears when the test injects concurrent ops via a second daemon."

## Run against a real daemon, not mocks

Playwright (https://playwright.dev/) drives the Svelte app in a browser against a real `txtodod`
spawned on a `tempfile::tempdir()` workspace, seeded with a fixture `todo.txt`. The Rust side
behind Tauri commands is a thin gRPC proxy, so either expose the same commands through a test
bridge or run the daemon and drive it directly. The point is real reconciliation, real file bytes,
and a real second daemon for the conflict case — not a stubbed reducer.

- Each scenario asserts the on-disk bytes, not just the DOM, where the plan says "the file changes
  only that line" (byte-diff the before/after file).
- Notes creation asserts lazy creation (§3.2.4): first keystroke creates the `ref:` directory and
  `notes.md`.
- Conflict sheet: a second daemon injects concurrent ops so `needs_review` arrives; assert the
  banner and sheet variants (M4's three).

## Acceptance

- The five scenarios from the plan pass, each against a fresh seeded workspace.
- Suite wired into the gate/CI and green on push.
