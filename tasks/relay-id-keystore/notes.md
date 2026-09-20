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
