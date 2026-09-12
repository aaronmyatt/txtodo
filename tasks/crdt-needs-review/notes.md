# Same-word edit detection sets needs_review; `txtodo conflicts list|resolve` (plan M4)

Depends on [crdt-loro-doc](../crdt-loro-doc/notes.md). Plan M4: "if two concurrent `EditText` ops
overlap in range, mark the task `needs_review` (stored in the op log as a local flag, **not** in the
file) and expose it via gRPC `Watch`."

## The trap: HLC cannot tell you what is concurrent

`Hlc` is a **total** order — every pair of stamps compares. That is what makes LWW deterministic,
and it is exactly why it cannot answer "were these two edits concurrent?". Two ops made offline on
separate devices still get ordered by wall time; nothing in the stamp says neither saw the other.

Concurrency is a *partial* order and needs version vectors. Loro tracks them
(<https://loro.dev/docs/tutorial/version>): two ops are concurrent iff neither's frontier dominates
the other's. So detection asks **Loro**, never the HLC. Write that down in the code, because the
`Hlc` field sitting right there on every `Op` is an inviting wrong answer.

## Detection

Runs after an import batch merges, never on the local-edit path:

1. For the touched tasks, collect `EditText` ops that are pairwise concurrent by Loro frontier.
2. Map each op's edits onto char ranges **in the merged text** (Loro's diff gives these; do not
   re-derive from the original offsets — the merge moved them).
3. Overlapping or adjacent ranges → flag. Adjacent counts: `foo|bar` interleaved from two sides
   reads as garbage even with no shared character.
4. Bound it. `MAX_REVIEW_FLAGS_PER_FILE`, asserted; past the cap, flag the file rather than every
   task, so a pathological merge cannot fill the table.

## Storage — local, additive

New migration `crates/txtodo-store/migrations/0002.sql` (additive, `PRAGMA user_version = 2`):

```sql
CREATE TABLE review_flags (
    file TEXT, task BLOB, raised_at INTEGER,
    mine BLOB, theirs BLOB,          -- description bytes of each side at flag time
    cleared_at INTEGER,              -- NULL while open
    PRIMARY KEY (file, task)
);
```

`mine` and `theirs` are stored, not recomputed. The alternative — replay the log excluding one
side's ops — is exact but costs a full re-merge per `conflicts list` call and gets slower forever.
Two description strings per flag is bounded by the cap above. **The merged text is not stored**: it
is whatever is in the file right now, which is the point.

Nothing about this touches the projection. A flagged file on disk is byte-identical to an unflagged
one; that is the "not in the file" rule and the external-edit goldens must stay green.

## Surfacing

- `Change` in `crates/txtodo-proto/proto/txtodo/v1/txtodo.proto` gains
  `repeated ReviewFlag review = 4;` — a new field number, never a renumber, and the regen check in
  CI must pass (`tasks/proto-grpc`).
- New rpc `ResolveConflict(ResolveRequest) returns (ApplyResponse)` rather than overloading `Apply`:
  resolving is "clear this flag by writing that text", and the daemon must do both atomically.

## CLI

- `txtodo conflicts` (alias for `conflicts list`) — line number, task, and a unified diff of
  `mine` vs `theirs`, `--json` like every other listing command (`tasks/cli-m2`).
- `txtodo conflicts resolve <line> mine|theirs|merged` — a closed enum in clap, not a string.
  `merged` means "what is in the file is right"; it writes **no** `EditText`, only clears the flag.
  `mine`/`theirs` write one `EditText` op back to the stored side and then clear.
- Daemon-only. With `--no-daemon` it must say so and exit non-zero, not silently show nothing.

## Open question for the human

Whose is "mine"? This device's edit, or the edit made by this *user* (same person, two devices)?
Device is trivial and what the code wants; user is what the words mean. Leaning **device**, with the
CLI labelling the sides by device name rather than the words mine/theirs in `list` output — the
subcommand keeps `mine|theirs` for muscle memory.

## Reading taken and as built (2026-09-12, agent)

- **"mine" = this device.** The CLI keeps `mine|theirs` for the subcommand; `list` labels sides
  by device. Recorded here, not confirmed by the human yet.
- `concurrent.rs` + `overlap.rs` landed as one `review.rs` in txtodo-crdt: with the frontiers an
  import names (`Imported { before, remote, ancestor, after }`), local edits are
  `diff(ancestor → before)` and the peer's `diff(ancestor → remote)`, both in ancestor
  coordinates, so overlap-or-adjacent is a plain range test. `mine`/`theirs` come from
  `doc.at(before)` / `doc.at(remote)`. No merged-text mapping was needed.
- Prerequisite discovered: an import is a *Loro update* import, so the daemon's mirror must be
  persisted and pairing must ship a snapshot (shared lineage). The store/proto/daemon/CLI halves
  follow.
