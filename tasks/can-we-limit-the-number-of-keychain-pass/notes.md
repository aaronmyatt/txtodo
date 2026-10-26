# At most one keychain prompt

## Why there are several (2026-10-17, agent triage)

- Only `txtodod` touches the keychain (`txtodo-sync/src/keystore_os.rs`, `keyring` 3 on the
  legacy login keychain). Every secret is its own item: service `txtodo`, account
  `device/<key>` for `device-static`, `relay-identity`, `group-epoch-N` and `device-signing`.
- Each item has its own access list. When `txtodod`'s code signature changes (an ad-hoc build, a
  cert change, a first install), macOS asks once per item: up to four prompts, at different
  times (startup reads two, the first sync session the group key, the first bundle export the
  signing key). `docs/keychain-runbook.md` says "up to four".
- Already fixed: repeat reads (`keystore_cache.rs`, once per key per boot), a retry joining the
  pending read (`keystore_timeout.rs`), startup waiting on an unanswered prompt
  (`keystore_setup.rs`).

## Options

- A. One vault item: every secret in one keychain item, read once into memory. One prompt per
  signature change, at the first read. No Apple account, access control unchanged.
- B. Read all items eagerly at startup: the prompts come together, but there are still four.
- C. A stable Developer ID signature / keychain access group: no prompt after the first, but needs
  a paid Apple team ID, an entitlement and a provisioning profile for a launchd binary, and
  `keyring` 3 cannot do access groups.
- D. An "any application" access list: no prompts ever, but any process of the user could read
  the group and signing keys silently: no better than a 0600 file.

## Decided 2026-10-17 (agent)

A. Reasons: the only option that turns "up to four" into "at most one" with no Apple account and
no weaker access; most of it (cache, single flight, waiting at startup) is built.

## Design

- `txtodo-sync`: `VaultKeyStore<S>` wraps the OS store. The vault is one item, `KeyId::Vault`
  (account `device/vault`): a version byte, then postcard of `(KeyId, bytes)` pairs.
  - The first call reads it once and keeps it in memory (wiped on drop); every later `get` of any
    key is answered from memory. The read holds a lock, so two keys asked at once still make one
    keychain read.
  - `put`/`delete` change the map and write the whole item back.
  - Migration is lazy: a key missing from the vault is read from its old item once (the old
    prompt, one last time), copied into the vault, and the old item deleted. After that the old
    items are gone and only the vault is read.
  - A vault that does not decode is an error, never overwritten.
- `txtodo-daemon`: `keystore_setup::os_key_store` wraps `OsKeyStore` in the vault, under the same
  timeout and cache. The probe item stays as it is (made and deleted by the same binary, so it
  never prompts).
- Runbook: "once per item (up to four)" becomes "once".

## Known gaps (planned)

- The first start after this ships can still ask for each old item once, as it moves into the
  vault. From then on: one prompt per signature change.
- Not verified on a real Mac here: whether "Always Allow" on the vault survives a self-signed
  reinstall (same open question as before, now for one item instead of four).

## As built (2026-10-17)

- `txtodo-sync` f134dc06: `VaultKeyStore<S>` and `KeyId::Vault` (appended, so the file keystore's
  postcard indexes hold). `txtodo-daemon` f50aa5e2: `os_key_store` is
  `Cached(Timeout(Vault(Os)))`. Runbook 1883f925.
- Tests run on a counting in-memory store (one inner read for every key; an old item moved once
  and deleted; a refused write leaves memory as the item is; a vault that does not decode is
  kept). No test touches the real keychain, by this repo's rule.
- Human check left: install a build with the vault (`just install-daemon`), start it in a
  terminal, count the prompts (expect one per old item this once), restart, expect none; rebuild
  ad-hoc (`cargo run`), expect one.
- Still open: whether "Always Allow" on `device/vault` survives a self-signed reinstall.
