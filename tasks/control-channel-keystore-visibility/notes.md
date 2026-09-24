## Goal

User report (2026-09-24): paired two devices, default workspace syncs both ways, but a
non-default workspace (this repo's own `todo.txt`) never showed up on the second device's
`txtodo workspace offers`/`list`.

## Diagnosis

From `/private/tmp/otherdevicelogs.log` (the second device's `txtodod` JSON log):

- `workspace_loader_started`/`workspaces_loaded`: 3 workspaces open locally (`default`, `/Users/oya`,
  `edusfere/backend`) — this repo was never among them, confirming the registry is per-device local
  state (`workspace_registry.rs`), never itself synced.
- `lan_shared_session_started` (`workspaces: 1`) three times — only the already-registered default
  workspace's group-sync connection ever ran.
- One `WARN control_channel_group_key_keystore_error`: `"OS keystore backend failed for Group(0):
  the OS keychain did not answer the read within 20s (a permission prompt nobody answered?)"`.

`control_session.rs::drive_control_session` calls `fetch_group_key` first and returns immediately
(never calling `send_all_offers`, never reaching `recv_control`) if that read fails — confirmed by
reading the source. So the one channel that carries `ControlMessage::Offer` for every
locally-registered workspace (`control_channel.rs`'s module doc: "every session ... send this
device's currently-registered workspaces as Offers") aborted before exchanging anything, in both
directions, on that connection attempt. This is symmetric with `ref:default-workspace`'s own
trailing note: "offers travel the relay control channel only" — so this single failure explains why
*no* non-default workspace (not just this one) can have reached the second device that session.

The 20s-timeout error message is exactly `keystore_timeout.rs`'s wrapper, built for
`ref:relay-id-keystore` (task `relay-id-keystore`, 2026-09-23) around the *relay identity* key —
this is the first evidence of the same OS-keychain-prompt problem hitting `KeyId::Group` reads via
`control_session.rs::fetch_group_key`, a different call site than the one that task instrumented.
`relay-id-keystore/notes.md`'s "Known gaps" already documents the mechanism: ad-hoc code signing
pins a keychain item's access list to the exact build's code hash, so every rebuild needs a fresh
"Always Allow" click, and a `launchd`-run daemon has nobody at the screen to click it. The signing
fix (a stable Developer ID) was already decided and deferred by a human on that task's last line —
**do not re-open that decision here**; this task is scoped to what's still missing given that
constraint stands: retried-forever failures that are invisible to the user.

Because `keystore_cache.rs` never caches an error (only successful reads, see commit 14bffbb), the
control channel *does* keep retrying every redial (`RESYNC_INTERVAL`, 15s) — it is not permanently
stuck the way a cached-failure bug would be. But every retry that hits the same unanswered prompt
costs a full 20s block with zero visibility: nothing on `Health`, `txtodo doctor`, or any client
UI says "workspace offers are stuck because the OS keystore hasn't been unlocked for this build" —
a user just sees an empty `workspace offers` list and has no way to tell that apart from "nothing
was offered."

## Scope

- In scope: making this failure observable (Health/doctor, client UI) so a user can self-diagnose
  and go grant keychain access, the same way `relay-id-keystore` made the relay-identity case
  observable via `doctor`'s `keystore` line.
- Out of scope, deliberately: re-deciding code signing (settled on `relay-id-keystore`'s last line),
  and the separate, already-tracked `ref:remote-workspace-mirror` gap (no auto-accept — even a
  successfully-delivered offer today needs a manual `txtodo workspace accept`).

## Known gaps

- Not reproduced against a live launchd daemon by this session — diagnosed from the attached log
  plus reading `control_session.rs`/`keystore_cache.rs`/`keystore_timeout.rs`, not run end to end.
- Whether the *first* device (the one whose workspace never arrived) also hit this, or only the
  second device's read of its own key, isn't distinguishable from one one-sided log — either side
  failing the read blocks the whole exchange (see Diagnosis).
