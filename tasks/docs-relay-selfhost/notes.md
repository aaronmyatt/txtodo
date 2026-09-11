# Self-hosting docs for the relay (plan M8, design §4.5/§4.6)

## Goal

Plan M8: "Relay is optional; self-hosting docs." Deliver two documents: a short user-facing
`README.md` section and a contributor-facing `docs/relay.md`, both stating plainly that the relay
is optional (design §4.5 "LAN alone is a complete system"), self-host-only (plan Q5 default), and
that it cannot read your data (design §4.6). Plan §5: each milestone updates `README.md` (user) and
`docs/` (contributor).

## Design

### The security sentence is the contract — exact wording

State what the relay stores and proves, nothing more, nothing less:

> The relay stores opaque, client-encrypted blobs keyed by device group and device, and forwards
> wake-up routing (APNs/FCM). It performs no encryption and cannot read op payloads or distinguish
> op types. It sees ciphertext and routing metadata only (design §4.6).

No overclaim ("your data is safe"), no underclaim ("end-to-end encrypted" is accurate only because
the client encrypts — say exactly that).

### docs/relay.md — contributor-facing structure

Each field must match the relay binary's actual `--help` (pinned by the drift test below); exact
flag names come from [relay-reference](../relay-reference/notes.md). The doc must cover:

- Build: `cargo build --release` in the `relay/` crate (package name per relay-reference).
- Run: listen address/port, data dir (blob store), retention window, max blob bytes per
  group+device, TLS cert/key for termination, wake-up forwarding endpoints (APNs/FCM — forward
  only, the relay never holds push tokens).
- A `deploy/systemd/relay.service` example unit (user-scoped, `Restart=on-failure`,
  `EnvironmentFile` for the flags), marked illustrative.
- TLS/reverse-proxy guidance: the relay terminates TLS itself or sits behind caddy/nginx; state the
  trust boundary — the proxy sees only ciphertext.
- The dumb-relay substitution: any S3 or WebDAV endpoint works (design §4.5); document the blob
  layout so an operator can roll their own.

### README.md — user-facing section

One short section: what the relay is (an optional sync mailbox for devices that can't reach each
other), that it is optional (LAN alone is complete), that it cannot read your list (design §4.6),
self-host-only per the Q5 default, and a pointer to `docs/relay.md`.

## Placement/dependencies

- `README.md` (user), `docs/relay.md` (contributor), `deploy/systemd/relay.service` — none frozen.
- No new crates or deps; docs-only. The only executable artifact is the `--help` assertion test,
  living in the relay crate's `tests/`.

## Edge cases & invariants

- The doc must not claim a public relay exists or will exist (Q5 default self-host-only).
- Retention/blob-cap numbers in the doc are examples only and marked as such — real defaults live
  in the binary; the drift test pins flag *names*, not default *values*.
- "Optional" is asserted in both docs: a reader must never conclude the relay is required for sync.

## Acceptance

- `docs/relay.md` covers build, run flags, systemd unit, TLS/reverse-proxy, retention + blob cap,
  and the S3/WebDAV substitution.
- `README.md` section exists and states optional + cannot-read-your-data + self-host-only.
- Drift test: relay `--help` output matches the flags the doc describes; a renamed/removed flag
  fails the test, so doc and binary cannot silently drift.

## References

- plan M8 and §5 (txtodo-implementation-plan.md), design §4.5/§4.6 (txtodo-design.md)
- Sibling: [relay-reference](../relay-reference/notes.md), [relay-converge-test](../relay-converge-test/notes.md)
- systemd units: https://www.freedesktop.org/software/systemd/man/latest/systemd.service.html
