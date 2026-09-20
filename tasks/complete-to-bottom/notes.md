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
