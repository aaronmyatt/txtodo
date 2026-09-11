# Conflict banner and review sheet for needs_review (plan M7, plan §4.7)

## Goal

When the daemon's `Watch` stream carries ops that flag a task `needs_review`, show a conflict
banner and a review sheet. The user-visible guarantee is design §4.7: txtodo never silently loses
something you typed — so the UI *offers*, the human *picks*.

## Design

- M4 mechanics: after a merge, two concurrent `EditText` ops overlapping in range mark the task
  `needs_review` — a local flag stored in the op log, not the file, exposed via `Watch`.
  `txtodo conflicts resolve <line> mine|theirs|merged` clears the flag by writing the chosen text
  as a new op. The sheet is the GUI for those three variants.
- Review sheet: the task text plus a char-level diff between mine and theirs (core `diff_text`,
  design §3 `diff.rs`) so the human can see the overlapping edit. Resolving calls `Apply(Edit)` with
  the chosen text — the same path as the popover, so attribution, sync, and history are automatic.
- Banner: a count of pending `needs_review` lines; clicking opens the sheet. Dismissing the banner
  must *not* clear the flag — clearing is a resolve action only, and only via a new op. Same
  spirit as §2.6/§3.2.6: no silent decisions.

## Acceptance

- Conflict sheet appears when the test injects concurrent ops via a second daemon.
- Resolving (`mine` / `theirs` / `merged`) writes the chosen line and clears the flag.

Refs: plan M7 (txtodo-implementation-plan.md), design §4.7 and §3 `diff.rs` (txtodo-design.md),
M4 `txtodo conflicts` (txtodo-implementation-plan.md M4).
