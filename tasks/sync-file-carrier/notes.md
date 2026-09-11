# File-carrier transport: append-only sync/<device-id>.ops in a user-chosen folder (plan M8)

Design §4.5 ("File carrier" row): each device appends ops **only to its own file**
(`sync/<device-id>.ops`) so dumb file sync (Syncthing / Dropbox / iCloud) never conflicts; devices
ingest each other's files. Plan M8 acceptance: two daemons sharing a directory, no network,
converge after each writes its ops file. §4.6: the folder sees only ciphertext.

## Own-file-only is the whole trick

- Write path: append frames to `sync/<device-id>.ops`, never touch another device's file. Two
  devices never write the same file ⇒ no merge conflict. Enforce with a check that the path's
  device id equals our own before any write.
- File contents are the encrypted `Ops` frames ([sync-crypto-envelope](../sync-crypto-envelope/notes.md))
  — ciphertext on disk, so a cloud folder learns only "this device wrote N bytes".

## Framing, because file sync is not a message channel

Append a length-prefixed frame, not a bare blob: a partial sync may deliver a half-written tail.
Ingestion skips an incomplete trailing frame and re-reads when more bytes land. Every op carries
`op_id`; import dedupes on `ops.op_id UNIQUE`, so re-reading a file or a duplicated chunk is
idempotent.

## Carrier shape

Fits the existing `trait Link` from [sync-lan-transport](../sync-lan-transport/notes.md): `send`
appends a frame to our file; `recv` polls the folder for other devices' files. The session machine
stays socket-free. Folder comes from `--sync-dir` / `config.toml`; validate it is a writable real
directory (external input → error, not assert). Append-only forever is unbounded: rotate
`sync/<device-id>-<n>.ops` at `MAX_OPS_FILE_BYTES`; readers scan all files in order, no manifest.

## Tests

- Two in-process carriers over one temp dir converge, no network, each writing only its own file.
- Re-reading the same `.ops` file imports nothing twice; a truncated trailing frame is skipped then
  completed on the next read.
- A device never writes to another device's file.
