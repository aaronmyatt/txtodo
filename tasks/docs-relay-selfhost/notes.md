# Self-hosting docs for the relay (plan M8)

Plan M8: "Relay is optional; self-hosting docs." Design §4.5: "The reference relay is a single
Rust binary; any S3 or WebDAV endpoint also works as a dumb relay." Plan §5 Docs: every milestone
updates `README.md` (user-facing) and `docs/` (contributor-facing).

## Default stance: self-host only

Q5 ("public relay or self-host only?") is still open; the plan's default is **self-host only**. The
docs describe running `relay/` yourself and say so plainly — no public relay infra is described or
implied. Relay stays optional: LAN alone is a complete system.

## Two audiences, two documents

- `README.md`, user-facing, one short section: what the relay is, that it is optional, that it
  cannot read your data (design §4.6), and a pointer to the full guide.
- `docs/relay.md`, contributor-facing: build (`cargo build -p …`), the env/flags
  ([relay-reference](../relay-reference/notes.md) port / data dir / retention), a `deploy/systemd`
  example unit, TLS/reverse-proxy guidance, and the retention + blob-cap knobs.

## The security sentence must be exact

State what the relay stores and proves: ciphertext blobs keyed by group + device, wake-up routing,
nothing else. Do not overclaim ("your data is safe") or underclaim ("end-to-end encrypted" is
accurate only because the client encrypts — the relay does no crypto).

## Tests

- Docs-only, but the runnable commands in `docs/relay.md` should be copy-pasteable: the relay
  binary's `--help` output is asserted to match the flags the doc describes, so the doc cannot
  silently drift from the binary.
