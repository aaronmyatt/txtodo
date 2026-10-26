# The daemon notifies when no desktop is open

## Goal
- Asked by the human 2026-10-11: with the desktop closed, the user still hears about tasks edited,
  deleted or completed on another device.
- The desktop, when open, gives the richer notice (it also covers local agents and editor saves,
  skips while focused, has a Settings toggle). While it is connected, the daemon stays quiet.
- Follows tasks/change-notifications (done): `txtodo_core::notice::Debouncer`, Watch ops with a
  `source`, the desktop's `notify.rs`.

## Design
- What counts: only ops whose `source` is `sync` (made on another device). Local agents, the CLI
  and editor saves stay a client-only notice, as today. Same kinds and verbs as the clients
  (`notice::verb_of`), same 3 s quiet / 15 s max burst.
- Core: `Debouncer` today drops one own source. The daemon needs the reverse: keep only `sync`.
  Add a constructor for that (`Debouncer::only_from("sync")` or similar) beside `new`.
- Who owns the notice: a new `bool shows_notices = 3` on `WatchRequest` (additive, old clients send
  false). The daemon counts open Watch streams that set it; while the count is above 0 it shows
  nothing. The burst is checked at show time, not at push time, so a desktop that opens mid-burst
  still wins.
- The desktop sets `shows_notices` on its Watch whenever it is connected, toggle on or off: toggle
  off means quiet from both, not "hand it to the daemon". The TUI does not set it: a toast in a
  terminal the user may not be looking at is not a reason to hold the OS notice.
- OS notifier in the daemon: `notify-rust` (macOS via mac-notification-sys, Linux via D-Bus,
  Windows toast). Best effort: no session bus (headless Linux, a relay host) or a refused
  permission logs once at warn and turns daemon notices off until restart. Behind a small
  `Notifier` trait so tests record notices instead of showing them.
  Ref: https://docs.rs/notify-rust
- Off switch: `txtodo notices on|off` writes or removes a marker file in the daemon's state dir;
  the daemon checks it at show time (no RPC, no config file). `TXTODO_NO_NOTICES=1` also turns it
  off, and the test harnesses set it.
- Wording: the notice says which device when the op's `device` maps to a paired device name
  (`DeviceList`), else "another device".

## Decided 2026-10-11 (agent)
- Supersede signal. Options: (A) `WatchRequest.shows_notices` field; (B) the daemon guesses from a
  Watch stream with no paths, or from the desktop process being alive. Pick A: explicit, additive,
  one field; B breaks when the TUI also watches every list (it does now).
- Notifier. Options: (A) `notify-rust`, one crate for three OSes; (B) shell out to `osascript` /
  `notify-send` per OS. Pick A: no text quoting into a script (the injection risk the desktop
  decision named), and Linux/Windows come free. New dependency: `cargo deny check` before landing.
- Scope "across devices" literally (`sync` only). Widening to agents later is one line in the
  filter.

## Open questions
- macOS: does a launchd LaunchAgent binary with no app bundle get to show a notice on current
  macOS? `mac-notification-sys` borrows a bundle id; if that is refused, fall back to `osascript`
  with the text passed as an argv item (not spliced into the script). Check on a real Mac first,
  before building the rest.
- An ADR: the daemon gains user-facing output (it was headless). Write one, short.

## Decided 2026-10-11 (agent, picking this line up): notifier changed to argv shell-out
- The plan above picked `notify-rust` pending a macOS spike nobody ran. Picked instead: macOS
  `osascript` and Linux `notify-send`, the title and body passed as argv items (the script reads
  `item 1/2 of argv`), so no text is ever spliced into AppleScript: the injection risk that made
  the plan avoid shelling out does not apply. No new dependency, and a LaunchAgent runs in the
  user's GUI session, where `osascript` may post. ADR 0041.

## As built (2026-10-11)
- core `Debouncer::only_from("sync")`; proto `WatchRequest.shows_notices` (3); daemon `notices.rs`
  (`Hub` fed from `FileActor::broadcast`, a timer thread, `Notifier` trait, `OsNotifier` with argv,
  `Owned` stream wrapper holding the owner guard, `install_os` from `main` for the global daemon
  only); desktop sets `shows_notices: true`, the TUI false; `txtodo notices on|off|status`.
- Tests: `notices_tests.rs` (recording notifier: shows once quiet, sync only, held by an owner and
  by the marker, a failing notifier turns it off, text passed as argv).
- `.cargo/config.toml` sets `TXTODO_NO_NOTICES=1` so two test daemons syncing never pop a notice.
- Known gaps: the notice says "another device", not the device's name (DeviceList lookup not
  wired); not shown on a real Mac yet (the human check line); the installed daemon needs
  `just install` (or the next release) to get it.
