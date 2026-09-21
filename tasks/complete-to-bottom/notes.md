# complete-to-bottom

## Goal

Decided 2026-09-20 (it replaces the two open lines "incomplete tasks should always be sorted to the
top" and "complete `x ` tasks should always be sorted to the bottom", which asked the same thing).
Completing a task moves its line to the bottom of its own file, once, in every client. It is in the
file, so every client agrees and it shows in diffs. The other choice was to sort only in the views,
which leaves the file alone; it was rejected because `cat`, todo.sh and other editors would show
the file unsorted.

## Design

- It is one move at the moment of completing, not a rule kept true on every write. Open lines are
  never touched. A done line you drag somewhere else afterwards stays there.
- The daemon does it, in `Mutation::Complete`, as one batch with the move (`MoveToEnd`, which
  exists). Every client sends the same mutation, so they agree without each one reimplementing it.
  A line that is already last does not move.
- "Its own file" means the file the line is in. A line in `tasks/<slug>/todo.txt` goes to the
  bottom of that file, never the root list.
- Reopening moves the line to the end of the open block, just above the first done line, so open
  lines stay on top. Its old position is not remembered. This is my choice; say if you want the
  line to stay put on reopen instead.
- An external edit is never moved. If you type `x ` in an editor and save, the daemon reconciles it
  as the edit it is. Only the Complete action moves a line.
- Done lines already in the middle of old files are not reflowed. Apply this going forward.
- The completed line's row: the TUI and the desktop keep the selection on the same row, so the
  next open task slides up under it instead of the cursor chasing the moved line.

## Known gaps

- `txtodo do` runs an archive after completing, which re-sorts every done line in the file. That
  fights "a hand-moved done line stays". The CLI line stops that; `txtodo archive` stays for anyone
  who wants the full sort.
- The desktop is an in-place editor. If a tick writes `x ` as text, it is an external-style edit
  and would not move. The desktop line checks this first.
- The backlog skill and the repo CLAUDE.md both tell agents to archive after each completion. That
  becomes unneeded and misleading once this lands.

## As built (2026-09-20)

- Daemon `e2d5b96`: `Mutation::Complete` appends a `Move` after the last task, in the same batch.
  No move when the completion changed nothing or the line is already last, so neither adds an op.
  `9a558e2`: two real-daemon tests show a hand-typed `x ` and a hand-moved done line stay put (no
  code needed: the reconciler never goes through `Mutation::Complete`).
- CLI `399cdc0`: `do` moves only what it completed (`edit.rs::move_to_end`); `-A` leaves even that
  in place; `archive` is unchanged. The real-daemon test now expects `c, d, x:a, x:b`.
- TUI `5cde47c`: tests only. Space already sent `Complete` and `rebaseline` already kept the row.
- MCP `cbc4da7`, `6f860b7`: the `todo_complete` description says the line moves; the returned row
  has the new line number and the same id.
- Desktop `e968078`: the main list has no tick (a human types `x `, a text edit, which by design
  does not move). The detail view's Mark done sends `Complete`, so the view now follows its task by
  id; pinned to a line number it would have shown another task.
- Playbook 3.2 and `AGENTS.md`.

- Reopen (2026-09-21): `Mutation.Reopen` is on the wire (`a3727a5`). The daemon (`mutation_reopen.rs`) clears the `x`, restores `(X)` from `pri:X`, and moves the line above the first done line; no move when it is already there, and nothing for a line that is not done. It sorts the x-clearing ops ahead of the priority ops: `change_ops` puts the priority first, which is right for completing but would fold a restored `(B)` back into `pri:B` and lose it. MCP's `todo_uncomplete` sends it (`28bd62d`). No TUI or desktop action un-completes today, so neither sends it.

Open:

- The TUI's Space always sends Complete; on a done line it does nothing. Sending Reopen there is an optional follow-up.
- In daemon mode the CLI still sends `Edit` plus `MoveToEnd` (or one `Replace` under Sidecar), not `Complete`. The file is the same; the op log says edit and move, not complete.
- The whole daemon test suite was not re-run after the `Complete` change, only the tests that complete a task.
- The global skill file and the root `CLAUDE.md` copy still carry the old 3.2.
