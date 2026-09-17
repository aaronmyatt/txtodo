# daemon-device-set-identity

## Summary

Device identity, keystore and group key move from per-`Workspace` (`workspace.rs:35-54`) to one
device-set-scoped home (ADR 0021), with a migration story for existing per-workspace group keys —
a real prerequisite for pairing once covering every workspace instead of once per project.

## As built

`device_identity.rs::DeviceIdentity` (device id, sync group, keystore, pairing registry)
constructed once per `txtodod` process and shared by every workspace it opens; `Workspace` keeps
every accessor's exact signature, delegating underneath (`b644790` txtodo-store: new
`IdentityStore`, its own `identity.db`; `de2fdfc` txtodo-daemon: the wiring).

**Migration story**: a fresh mint, no automatic adoption of a pre-existing workspace's own
group/keystore (documented in `device_identity.rs` — no released users yet to migrate).

Verified with every real two-daemon test in the crate (`pairing_lan`, `lan_loopback_converge`,
`lan_discovery`, `file_carrier_converge`, `nested_ref_sync`, `relay_converge`, +179 unit tests)
plus all `txtodo-store` tests; `pairing_relay.rs` hit its own pre-existing, self-documented
public-relay flake unrelated to this change.

**Not done**: the sync `Link` is still one per open workspace — `daemon-shared-sync-link` is
next.
