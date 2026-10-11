# 0041 — The daemon shows a notice for changes from another device

- Status: accepted 2026-10-11 (decided by the agent on the owner's standing instruction to decide
  `@human` lines; plan in `tasks/daemon-notices/notes.md`)
- Date: 2026-10-11
- Deciders: agent, on the owner's standing instruction

## Context
Clients notify on another client's changes (`txtodo_core::notice`, task change-notifications), but
only while one runs. With the desktop closed, a task completed on the other Mac goes unseen. The
daemon is always running, and until now it never spoke to the user.

## Decision
- The daemon shows one OS notice per burst of ops whose `source` is `sync` (made on another
  device), with the same debouncing and words as the clients (`Debouncer::only_from("sync")`).
- A client that shows its own notices says so with `WatchRequest.shows_notices` (an additive bool).
  While any such Watch stream is open the daemon shows nothing; it checks at show time.
- The notifier shells out with the text as separate argv items, never spliced into a script:
  macOS `osascript -e 'on run argv' -e 'display notification (item 2 of argv) with title (item 1
  of argv)' -e 'end run' <title> <body>`, Linux `notify-send <title> <body>`, elsewhere nothing. A
  failure logs once at warn and turns daemon notices off until restart.
- Off switch: `txtodo notices off` writes a marker file in the daemon's state dir; `on` removes it;
  `TXTODO_NO_NOTICES=1` also silences it (test harnesses set it).

## Options
- Notifier: `notify-rust` (one crate, three OSes) was the first plan; rejected for now because on
  macOS it borrows another app's bundle id and whether a bare LaunchAgent binary may post at all
  was unverified. `osascript` runs in the user's GUI session from a LaunchAgent and needs no new
  dependency. Revisit if notices need actions or icons.
- Supersede signal: guessing from "a Watch stream with no paths" breaks now the TUI watches every
  list too; an explicit field is one bool.

## Consequences
- Good: changes from another device are seen with every client closed.
- Bad: the daemon gains user-facing output; a headless host must not try (no `osascript`, no
  session bus: the first failure turns it off).
- Neutral: scope is other devices only; local agents and editor saves stay a client notice.
