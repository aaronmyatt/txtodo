# txtodo bundle export|import — air-gapped sneakernet carrier (plan M8, design §4.5)

## Goal

`txtodo bundle export` / `txtodo bundle import` is the Sneakernet row of design §4.5: one
self-contained file, git-bundle style (https://git-scm.com/docs/git-bundle), carried by hand (USB
stick) to bootstrap or update an air-gapped device. It moves the entire op state — snapshot, op
tail, per-file hashes, manifest — so the receiving device ends up byte-identical to the sender with
no network involved. Design §4.5 gives only "git-bundle style"; everything below is the concrete
breakdown.

## Design

### The seam — daemon gRPC, not a CLI store read

`budgets.json` `allowedDeps["txtodo-cli"] = ["txtodo-core", "txtodo-proto"]` — no `txtodo-store`.
The op log lives in SQLite (`.txtodo/oplog.db`, plan M3 §Store: one row per op
`(hlc, device_id, principal, op_bytes, signature)`, snapshots every N ops) and is reachable only
through `txtodo-daemon`. So the bundle body is produced/consumed by the daemon and streamed over
the local gRPC socket; the CLI owns the command UX, the passphrase prompt, and a bounded copy. Same
"daemon is the only thing behind the surface" shape as `txtodo-mcp` (design §6.1).

```rust
// crates/txtodo-proto/proto/txtodo/v1/txtodo.proto — additions (proto is not frozen)
message FileHash { string file = 1; bytes blake3 = 2; }               // per synced document
message BundleManifest {
  uint32 version = 1;          // wire/format version, from day one (same rule as sync frames)
  string schema_version = 2;   // store migrations version (meta table)
  bytes device_id = 3;         // 32-byte Ed25519 identity key (DeviceId)
  repeated FileHash files = 4; // every todo.txt/done.txt/notes.md under the root
  uint64 snapshot_seq = 5;     // newest snapshot included
  uint64 tail_start_seq = 6;   // first op row streamed after the snapshot
}
message BundleChunk { bytes data = 1; }   // bounded frame (64 KiB); never one giant blob
message BundleExportRequest { bytes passphrase = 1; }  // loopback only; KDF input, not stored
message BundleImportResponse { uint64 ops_imported = 1; repeated string files = 2; }

service Txtodo {
  // …existing ListFiles/GetFile/Watch/Apply/History/Undo/Checkout…
  rpc BundleExport(BundleExportRequest) returns (stream BundleChunk);
  rpc BundleImport(stream BundleChunk) returns (BundleImportResponse);
}
```

- Export order is fixed: manifest first (import validates before reading payload), then the
  snapshot blob, then op rows `(seq, op_id, hlc_wall, hlc_counter, device, principal, file, kind,
  payload, signature)` in `seq` order. Design §4.4 keeps full history by default, so the tail can
  be large — stream it, never buffer it.
- The whole stream is wrapped: Argon2id (https://docs.rs/argon2) passphrase KDF →
  XChaCha20-Poly1305 (https://docs.rs/chacha20poly1305) sealed blob. The physical medium is the
  sneakernet boundary; the passphrase is the second line for a lost USB stick.

### Key material — 2 options, recommend key-free

Design §4.5 does not say whether the bundle carries the group key.

- **Key-free (recommended):** the bundle carries *data only*. Key admission happens out-of-band via
  §4.6 pairing (QR / SAS — proximity works air-gapped), then the bundle is imported. The passphrase
  wrap is confidentiality/integrity at rest, not key distribution. No key bytes in the manifest.
- **Key-in-bundle:** the group key travels inside the passphrase wrap so import also admits the
  device. Simpler for the receiver, but the bundle becomes a bearer of the group secret.

I'd take key-free: §4.6 already has a key-admission path that works without a network, and it keeps
the bundle from being a key-escrow artifact. Flag once to the human; default key-free.

## Placement/dependencies

- New `txtodo-proto` messages + two streaming RPCs (proto is not frozen; the `proto-grpc` fence
  regenerates the committed `generated/` output — constitution §6 generated-artifact rule).
- `crates/txtodo-daemon/src/bundle.rs`: `BundleExporter`/`BundleImporter` over `txtodo_store`
  reads; `crates/txtodo-cli/src/bundle.rs`: the two subcommands, passphrase prompt (`rpassword` or
  `--passphrase-file -` for stdin), bounded 64 KiB copy loop.
- New deps `argon2`, `chacha20poly1305` need human sign-off + a `cargo deny` pass (`deny.toml`
  frozen). `blake3` is already in the tree (projection hashes, `handle.rs`).

## Edge cases & invariants

- Import is all-or-nothing: version mismatch, schema mismatch, one bad Ed25519 signature, one bad
  blake3 hash, truncation, or a wrong passphrase each yields a typed error naming the failing stage
  — never a partial insert. Assert `ops_imported == 0` and no projection write on any failure path.
- The manifest is written first so import can reject before allocating; every claim in it (counts,
  hashes) is re-checked against the actual stream, never trusted.
- Re-import of an already-present bundle is idempotent via `op_id` uniqueness (`ops` table
  `op_id BLOB UNIQUE`) — duplicates are skipped, not re-applied.
- `device_id` in the manifest is the sender's; import does not adopt it — the importer keeps its
  own identity (design §4.6: identity is per-device, generated on first run).

## Acceptance

- Export on A, import on a fresh B ⇒ identical file bytes and identical op-log state (same `seq`,
  `op_id`, HLCs, principals).
- One flipped byte anywhere fails import with a distinct error and leaves zero partial state.
- Wrong-passphrase import fails without writing any state; the clear manifest carries no key
  material (cross-checked by [security-m8-review](../security-m8-review/notes.md)).
- A nested-`ref:` workspace (§3.2) round-trips reproducing the whole tree.
- `cargo deny` passes for the two new deps.

## References

- plan M8 (txtodo-implementation-plan.md), design §4.4/§4.5/§4.6 (txtodo-design.md)
- git-bundle: https://git-scm.com/docs/git-bundle
- https://docs.rs/argon2 · https://docs.rs/chacha20poly1305 · https://docs.rs/blake3
- Sibling: [sync-crypto-envelope](../sync-crypto-envelope/notes.md), [security-m8-review](../security-m8-review/notes.md)
