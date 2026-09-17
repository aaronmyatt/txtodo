# File-carrier transport: append-only sync/<device-id>.ops in a user-chosen folder (plan M8, design §4.5)

## Goal

The "File carrier" row of design §4.5: each device appends ops **only to its own file**
(`sync/<device-id>.ops`) in a shared folder, so dumb file sync (Syncthing / Dropbox / iCloud Drive)
never conflicts; devices ingest each other's files. Plan M8 acceptance: two daemons sharing a
directory, no network, converge after each writes its ops file. §4.6: the folder sees only ciphertext.

## Design

Own-file-only is the whole trick: two devices never write the same file ⇒ no merge conflict. The
carrier is another `trait Link` impl ([sync-lan-transport](../sync-lan-transport/notes.md)) — the
session machine stays socket-free.

```rust
// crates/txtodo-sync/src/carrier.rs
pub struct FileCarrier { dir: PathBuf, device: DeviceId }
impl Link for FileCarrier {
    fn send(&mut self, frame: Frame) -> Result<(), LinkError>;   // append frame to OUR file
    fn recv(&mut self) -> Result<Frame, LinkError>;              // poll folder for OTHER devices' files
}
// write path: assert the target path's device id == our own BEFORE any write (never touch others').

// crates/txtodo-sync/src/frame.rs — length-prefixed append framing
pub struct AppendFrame { len: u32, body: Vec<u8> }   // partial sync may deliver a half-written tail
```

- **Framing, because file sync is not a message channel**: append a length-prefixed frame, not a
  bare blob. Ingestion skips an incomplete trailing frame and re-reads when more bytes land.
- **Idempotence**: every op carries `op_id`; import dedupes on `ops.op_id UNIQUE`
  ([sync-protocol-frames](../sync-protocol-frames/notes.md), `txtodo-store`), so re-reading a file
  or a duplicated chunk imports nothing twice.
- **Ciphertext on disk**: file contents are the encrypted `Ops` frames from
  [sync-crypto-envelope](../sync-crypto-envelope/notes.md) — a cloud folder learns only "this
  device wrote N bytes".
- **Rotation**: append-only forever is unbounded, so rotate `sync/<device-id>-<n>.ops` at
  `MAX_OPS_FILE_BYTES`; readers scan all files in order, no manifest.
- **Config**: folder from `--sync-dir` / `config.toml`; validate it is a writable real directory
  (external input → error, not assert).

## Placement/dependencies

- `crates/txtodo-sync/src/carrier.rs` + `src/frame.rs`; config plumbing in the daemon + `txtodo`
  CLI (`--sync-dir`). No new deps — reuses `Frame`/`Link`/`postcard` from
  [sync-protocol-frames](../sync-protocol-frames/notes.md).

## Edge cases & invariants

- A device never writes another device's file (asserted before every write).
- Incomplete trailing frame: skip now, complete on next read — never desync.
- Re-read / duplicated chunk: idempotent via `op_id` dedupe.
- Invariant: every byte in the folder that is not our own frame is external input, validated and
  never asserted.

## Acceptance

- Two in-process carriers over one temp dir converge, no network, each writing only its own file.
- Re-reading the same `.ops` file imports nothing twice; a truncated trailing frame is skipped then
  completed on the next read.
- A device's write to another device's file is refused.

## References

- design §4.5 (File carrier row), §4.6; plan M8 acceptance.
- [sync-protocol-frames](../sync-protocol-frames/notes.md) ·
  [sync-lan-transport](../sync-lan-transport/notes.md) ·
  [sync-crypto-envelope](../sync-crypto-envelope/notes.md).

## As built

`crates/txtodo-sync/src/{append_frame,carrier,carrier_error}.rs` (`FileCarrier` as a `Link` impl)
built and tested (4/4 acceptance tests); `crates/txtodo-cli` config `sync_dir`/`--sync-dir`/env
wired. `op_id`-level dedupe into `txtodo-store` deliberately deferred — see this task's own
`todo.txt` remaining `@store` item.
