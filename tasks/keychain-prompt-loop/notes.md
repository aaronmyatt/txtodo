# keychain-prompt-loop

## Goal

2026-09-29, after `just install`: an endless run of macOS keychain modals. The daemon log showed
`txtodod` starting every 20 s (launchd `runs = 25`, `last exit code = 1`), each start ending with
`sync keystore: OS keystore backend failed for DeviceStatic: the OS keychain did not answer the
read within 20s`.

- Startup reads `device/device-static` from the login keychain. The new build is signed with the
  "txtodo Self-Signed" cert; the item's "Always Allow" list most likely held only earlier
  `just install-daemon` builds (plain `cargo install`, a new ad-hoc signature each build), so
  macOS asked.
- `keystore_timeout.rs` gives each keychain call 20 s on its own thread. At startup that error was
  fatal: the daemon exited 1, launchd's `KeepAlive` started a new process, and the new process
  asked again. Nobody can type a password and pick Always Allow on a modal that is replaced
  every 20 s.
- Later reads had the same shape at a slower pace: each retry spawned a fresh keychain call, so
  a second prompt, instead of waiting on the one already up.

## Design

- A: `TimeoutKeyStore::get` keeps one pending read per key. A caller that times out gets the
  same error as before, but the read stays pending; the next `get` of that key waits on it
  instead of starting another keychain call. One prompt per key per process.
- A: startup (`DeviceIdentity::finish_open`) retries a timed-out load until the prompt is
  answered, logging `keychain_prompt_pending` every 20 s, instead of exiting. Bounded at a day,
  then the old error. Any other keystore error still fails at once.
- B: every local path that installs a `txtodod` that touches the real keychain signs it with the
  same "txtodo Self-Signed" cert and identifiers as `just install` and release.yml, so "Always
  Allow" given once holds across rebuilds: `just install-daemon` now runs `scripts/install-local.sh`
  without the desktop app; `just stage-desktop-sidecar` signs the staged sidecar.

## Known gaps

- While startup waits, the socket is not bound, so a client gives up after its spawn timeout
  (120 s) with "daemon did not become ready"; the prompt is still on screen.
- `cargo run` / `target/debug/txtodod` builds are still ad-hoc signed; against the real keychain
  they prompt after every rebuild (tests use `TXTODO_TEST_KEYSTORE_MEMORY=1`).
