# keystore-memory-fallback

Found reviewing the last 100 commits (`18b37ea^..HEAD`) on 2026-09-21.

## Goal

`--key-store auto` with an in-memory fallback shipped in `77b50dd`, and `06afecf` made
`txtodo doctor` hard-fail on it. Both the warning and the doctor row understate the damage by a
long way, and the test escape hatch that forces the fallback is reachable from a plain `cargo run`.

## What the fallback actually costs

`KeyId::Group(epoch)`, `DeviceStatic` and `DeviceSigning` live in the same keystore as
`RelayIdentity` (`crates/txtodo-sync/src/keystore.rs:22-34`). On the memory fallback every one of
them is reminted at each start, while the `DeviceId` in `identity.db` persists. So after one
restart the device presents the *same id* with *different* static and signing keys: every paired
peer's handshake breaks and all previously-sealed group epochs are unrecoverable.

`crates/txtodo-daemon/src/keystore_setup.rs:97-107` and
`crates/txtodo-cli/src/commands/doctor.rs:234-241` both say only "the relay identity will not
survive a restart". The honest sentence is "sync is dead and you must re-pair".

Rewording is the small half. The real fix is refusing to start relay or LAN sync at all when
`backend == "memory"` — a daemon that cannot keep keys has no business holding a sync group.

## The env escape hatch

`crates/txtodo-daemon/src/identity_setup.rs:50-53` short-circuits to `open_in_memory` on
`TXTODO_TEST_KEYSTORE_MEMORY` *before any logging*, so it is the one in-memory route with no
warning — unlike `on_auto_probe_failure`, which shouts.

`TXTODO_TEST_KEYSTORE_MEMORY` is set in `[env]` in `.cargo/config.toml:58-67`, which applies to
everything cargo runs in this repo, not just tests. `cargo run -p txtodo-daemon --bin txtodod`
therefore silently gets volatile identity keys — and doctor now hard-fails on exactly that. It is
also an unguarded runtime env check in the shipped binary: no `cfg(test)`, no debug-only gate,
which is what CLAUDE.md §2.3 means by "keep any test-only seam guarded so it can never ship enabled
to production".

Two ways out: gate it behind `#[cfg(debug_assertions)]` or a build-time cfg, or take it out of
`[env]` and set it in the test harnesses that spawn `txtodod`. The second is cleaner — the seam
stops existing in release builds at all.

## Open

`tasks/relay-id-keystore/notes.md` records a deferred signing decision. Worth re-reading before
starting: if signing keys move out of this keystore the first two lines change shape.
