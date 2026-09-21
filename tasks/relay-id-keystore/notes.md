# relay-id-keystore

## Goal

Decided 2026-09-20 (option A): `txtodod` defaults `--key-store` to `auto`, the OS keychain. The relay
node id that `txtodo doctor` prints must survive a daemon restart. Today launchd and the desktop
start `txtodod` with no `--key-store`, so the key lives in memory, the relay identity is minted
fresh on every start, and a relay `access.allowlist` entry goes stale each time. The other choice
(B) kept the relay secret unencrypted in the store: it works everywhere but is weaker at rest.

## What auto does today (checked in `keystore_setup.rs`)

- `auto` probes the OS keychain. If the probe fails it returns `KeyStoreError::AutoNeedsChoice` and
  never falls back to a file on its own initiative. That rule is called out by name in the keystore
  task notes.
- So a default of `auto` on a box with no keychain, such as a headless Linux server with no Secret
  Service, would stop `txtodod` from starting. That is the answer to the open question on headless
  Linux, and it is why the second line is a Decide.

## Design

- The default applies only when the flag is absent. An explicit `--key-store auto|os|file` behaves
  exactly as before, including the refusal.
- Recommended for the no-keychain case (A in the Decide): a defaulted `auto` runs with the in-memory
  store as it does today, and says so loudly, in the log and in `doctor`. This keeps the "no silent
  file fallback" rule and does not turn a working headless box into one that will not start.
- `doctor` names the backend and fails when the relay identity is in memory only, so a stale
  allowlist is visible before it bites.
- The desktop and the launchd unit pass no flag, so they pick up the new default without a change.

## Known gaps

- macOS may prompt for keychain access. If the prompt returns after every reinstall (the binary
  changes), that is a cost of A. The last line checks it.
- Checked by hand 2026-09-20 on macOS: **yes, the prompt returns after a reinstall.**
  - Run 1 (`txtodod --dir . --key-store os`): no prompt. It wrote `txtodo` / `device/relay-identity`
    to the login keychain. The binary that creates an item is trusted for it.
  - Rebuilt (one small edit, new ad-hoc CDHash), run 2: macOS asked "txtodod wants to use your
    confidential information stored in "txtodo" in your keychain" and wanted the login password.
  - Why: our builds are ad-hoc signed (`Signature=adhoc`, no team id), so the item's access list
    pins the exact code hash. Every new build is a stranger. "Always Allow" only lasts until the
    next build.
  - Cost of A, then: one password prompt per reinstall, per keychain item. Under launchd nobody may
    be at the screen to answer it; what the daemon does while the prompt sits there is not checked.
  - Way out, not done: sign with a stable Developer ID so the access list pins the signer, not the
    hash. Ref: https://developer.apple.com/documentation/security/keychain_services/access_control_lists
  - Two false starts first: a run with no `--key-store` is in memory and never touches the
    keychain, and a second daemon quits on the pidfile lock before it opens the keystore. Neither
    shows a prompt, and neither is a test.
  - Not in a runbook yet: there is none (`relay-kamal-deploy` line 7 is still open). Put it there.
- This unblocks the Kamal deploy (`relay-kamal-deploy`), which is low priority and still needs a
  droplet, DNS and secrets.

## As built (2026-09-21)

- Decided: A (in-memory + loud warning, not refuse to start) for the defaulted-no-keychain case —
  see the root `todo.txt` line's own decided note. Signing decision (last line) deferred by the
  human: live with the reinstall prompt for now, revisit when the app ships to others.
- `identity_setup.rs` (new, split out of `main.rs` for its file-length budget): `build_identity`
  now defaults `--key-store` to `Auto` instead of calling `open_in_memory` unconditionally; an
  explicit flag is threaded through unchanged.
- `keystore_setup.rs::resolve_key_store` gained a `defaulted: bool` param. Its `Auto` arm's probe
  failure now goes through `on_auto_probe_failure(reason, defaulted)` — a pure decision function so
  the defaulted-vs-explicit branch is unit-testable without a real OS keychain (`OsKeyStore::probe`
  has no test seam, this crate's own "never OS-keychain-reachability-dependent" rule for tests).
  `defaulted=true` logs a `tracing::warn!` and falls back to `MemoryKeyStore`; `defaulted=false`
  keeps returning `KeyStoreError::AutoNeedsChoice`, unchanged.
- **Real risk found and fixed while landing this**: flipping the no-flag default to `auto` means
  every real-daemon integration test across this workspace (`txtodo-daemon` itself, plus
  `txtodo-cli`/`txtodo-tui`/`txtodo-mcp`/`apps/desktop`, all of which spawn `txtodod` with no
  `--key-store` flag via `txtodo_daemon_launch::ensure_daemon` or a direct `Command::new`) would
  start probing the real OS keychain on every spawn — exactly the OS-keychain-reachability
  dependence this crate's own tests are supposed to never have, and a real flakiness/hang risk on
  CI and macOS (keychain access prompts). Fixed with `TXTODO_TEST_KEYSTORE_MEMORY=1`, set
  workspace-wide in `.cargo/config.toml`'s `[env]` next to the pre-existing `TXTODO_NO_SERVICE`
  (same mechanism: a spawned `txtodod` inherits it from whichever test process spawned it, plain
  process env inheritance — no per-crate test-harness edits needed). `main.rs::build_identity`
  checks it before doing anything else, short-circuiting straight to `open_in_memory` (never even
  attempts the real probe) when set and no `--key-store` flag was given; an explicit flag still
  wins over it. Verified against `crates/txtodo-daemon/tests/relay_node_id.rs`'s two tests (one of
  which spawns with no `--key-store` at all) and the crate's `tests/support`-based suite
  (`external_edits.rs` et al.) — all green, fast, no hang.
- Root `todo.txt` lines 1-3 closed this session. Still open (see this dir's `todo.txt`):
  - `txtodo doctor` failing on a `memory` backend (line, `@cli`) — blocked this session: needs
    `crates/txtodo-cli`, and the slice fence wouldn't hand over the lease while another concurrent
    session's uncommitted `tasks/default-workspace/*` files kept the repo tree dirty. Not started.
  - The relay-node-id-stable-across-restart line (`@daemon`, file backend in a test + OS keychain
    by hand): the file-backend half is already proven by the pre-existing
    `tests/relay_node_id.rs::a_daemon_with_a_relay_reports_a_stable_64_hex_node_id_across_a_restart`
    (built for task `cli-relay-node-id`, not this one, but it satisfies this line's file-backend
    half as-is). The "by hand on macOS" half needs a real run against the human's actual login
    keychain — not run, since that writes a real credential to the human's real keychain and this
    session had no explicit go-ahead for that.
  - The desktop sidecar-inherits-the-default line: confirmed by reading
    `apps/desktop/src-tauri/src/daemon/spawn.rs` (delegates to `txtodo_daemon_launch::ensure_daemon`,
    which passes no `--key-store`) — so it does inherit `auto` now. Not empirically proven (needs
    either a live desktop app run, which CLAUDE.md §2.3 says to ask before doing, or a
    `txtodo-daemon-launch` crate test, blocked by the same fence/dirty-tree issue as the doctor
    line above).
  - The denied/unanswered-keychain-read-fails-loud line: a denied read already fails loud (any
    `KeyStoreError` from `key_store.get`/`.put` propagates via `?` through `WorkspaceError` to a
    daemon-startup error, never silently swallowed into a fresh in-memory identity). An
    *unanswered* prompt (the human never responds) is a real hang, not a fast failure — the
    `keyring` crate's calls are blocking with no timeout, and fixing that is a bigger change (a
    timeout wrapper around every keystore read) this session didn't attempt. Flagged, not solved.
