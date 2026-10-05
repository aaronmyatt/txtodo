# tui-desktop-fixes-sync

## Goal
- The TUI keeps step with the desktop fixes filed 2026-10-05 (root todo.txt, the lines under the
  desktop revamp line).
- Logic both clients need lands once, in txtodo-core or the daemon, before either client builds it
  (ADR 0031: shared logic is not re-implemented per client).

## Map (desktop line → shared part → TUI part)
- "Add a line..." input at the top → none, unless the new line also lands at the top of the file
  (decide line) → move the `+ Add a line` row to the top (`state.rs:21-22`, `ui/list.rs:1-14`).
- Done items always move to the bottom → daemon: an Edit that makes a line done moves it like
  `Complete` does (`mutation.rs:279-292`) → TUI already sends `Complete` for x/Space; check every
  other path (typed `x `, parent "Mark done").
- Typed `x ` gets the completion date → daemon `edit_ops` (`mutation.rs:357-371`), daemon's own
  local date (ADR 0011) → nothing to send.
- Detail input box h-scrollbar hides the text, plus a breakdown view → long-line h-scroll is
  already in tui-text-editing (parent line field) → TUI line breakdown view, one field per row,
  like sqlite `.mode line`.
- ref path visible in the detail view; deleting it severs the link → none for hiding; daemon bug
  below → TUI hides `ref:` on the parent line the way it hides `id:` and keeps it on save.
- `X ` / `(a)` fixed for the user → one normaliser in txtodo-core, applied by the daemon on Add and
  Edit (today neither is a quirk; `strict_hint::LOWERCASE_PRIORITY` only hints) → nothing extra.
- New lines get today's date → daemon `add_ops` (`mutation.rs:323-355`) stamps nothing today; only
  the CLI does, client-side → nothing to send; the daemon stamps it.

## As found (2026-10-05)
- MCP `todo_add` refuses a leading date "because the daemon stamps it", but the daemon does not
  (`backend_args.rs:143,186`, `grpc_write.rs:90-101`). Lines added over MCP get no date.
- The desktop mock prepends today (`mock/logic.ts:33-37`), so mock and real daemon differ.
- Deleting `ref:` from a parent: the edit applies, the old dir is orphaned; the next write mints a
  new slug, and `-2` if the old dir still exists (`refdir_ops.rs:92-117`).
  - Possible bug: `resolve_ref_dir` checks only the document for collisions, not the filesystem
    (`refdir_ops.rs:122-147`), so it may predict the old slug while `ensure` claims `-2`.
- The TUI detail panel fixes `Level.dir` when it opens (`app_detail.rs:31-45`), so it does not
  follow a changed ref.

## Decisions (2026-10-05, agent; the human delegated the @human calls)
- **Dates come from the daemon's clock, not a new `today` field on Add/Edit.**
  - Why: ADR 0011 already says file dates are "the daemon's local date at write time". The
    daemon runs on the same device as the TUI or desktop, so its date is the user's date.
  - Gain: no wire change, and old clients get dates for free. MCP and the CLI agree too.
  - Cost: `Complete` still carries a client `today`, so there are two sources of "today". They
    only differ if the client and daemon run in different time zones. Not worth a wire change.
  - Rejected: a `today` field on Add/Edit. It needs a proto change and an old-daemon fallback, and
    it fixes a case (client and daemon in different zones) that does not happen today.
  - So the TUI "send today" line is dropped.
- **The top "Add a line" input puts the new line at the top of the file.**
  - Why: the input is at the top. A line that lands at the bottom, out of view, looks lost.
  - How: one Apply with Add, then a move before the first line. No new mutation field.
  - Cost: two ops per add instead of one. CLI `add` and MCP `todo_add` still append, so todo.sh
    behaviour is unchanged; only the two GUI inputs differ.
  - Rejected: appending (todo.sh style). It matches the CLI but fights the input's position.
- **Done lines move down on the write that marks them done. No timed sweep.**
  - Why: a sweep on two synced devices makes concurrent moves of the same lines, the kind of
    reorder the lab chaos runs still trip on. A sweep would also move lines under the cursor at
    random times.
  - On-write covers `Complete` (already), and an app Edit that makes a line done (typed `x `,
    desktop's x chip).
  - Known gap: a line marked done by hand in another editor stays where it is until someone runs
    `archive`. Moving lines in a file a human has open in vim would fight their editor.
- **The `X`/`(a)` fixer stays narrow.** Not a decide line, but it needs a rule:
  - Only `X ` at the very start of the line, and `(a) ` to `(z) ` at the start or after the done
    marker and its dates. Elsewhere `X` and `(a)` are just words.
  - Only on app writes (Add, Edit), never on an external edit and never as a side effect of sync.

## As built
- New lines dated (2026-10-05): `txtodo_core::line_fixes::stamp_created` (moved from the CLI's
  `add.rs`, which now calls it). The daemon stamps at the wire, `convert::parse_mutation(m, today)`,
  so only an `Add` a client sent gets a date; the `Add`s the daemon makes itself (a cross-file
  move's arriving line, `move_coordinator.rs`) keep theirs. `today` is `local_date::today_local()`
  (jiff, new daemon dep), read once per `Apply`.
  - MCP's "the daemon stamps it" refusal is now true; no MCP change needed.
  - Known gap: an added `x foo` (done marker, no date) is not done to the parser, so it gets a
    creation date in front of the `x`. The typed-`x` line fixes that.
  - The CLI e2e `daemon_autostart` tests need autostart on; they fail under
    `TXTODO_NO_AUTOSTART=1`, which is expected, not this change.
