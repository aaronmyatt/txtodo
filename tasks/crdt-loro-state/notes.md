# Replace `Vec<TaskState>` with the Loro-backed state, reconciler interface unchanged (plan M4)

Depends on [crdt-loro-doc](../crdt-loro-doc/notes.md) landing first.

## The good news: `reconcile` is already clean

`reconcile(old: &File, new: &File, path, mint) -> Reconciled` (daemon `reconcile.rs:30`) is pure and
never touches the state at all — it works on `txtodo_core::File`. So "keep the reconciler interface
unchanged" costs nothing. The swap is entirely inside `DocState` (daemon `state.rs`), which is the
M3 stand-in the module header already promises to replace.

## The actual blocker: `entries() -> &[Entry]`

`DocState` stores a `Vec<Entry>` and hands out a borrowed slice. A Loro-backed state cannot: the
lines live in a `LoroMovableList` + `LoroMap`, so there is no `&[Entry]` to lend. Every caller of
that slice is a place the internal shape leaked:

| caller | what it really wants |
|---|---|
| `fields.rs:26,50` | the line at index `i` |
| `mutation.rs:119,151,209` | the entry/line at index `i` |
| `mutation.rs:159,189`, `history.rs:90,102` | the nearest task id at or before index `i` |
| `history.rs:117,134` | the line for a `TaskId` |
| `state_tests.rs:28,29,136` | entry count and kind at an index |

That is four questions, not one slice. Narrow the surface **before** swapping the backing store —
as its own prior change, so the Loro commit is a pure substitution with the tests already green:

```rust
pub fn len(&self) -> usize;
pub fn entry_at(&self, i: usize) -> Option<Entry>;      // by value, not &[..]
pub fn line_of(&self, id: TaskId) -> Option<OwnedLine>;
pub fn task_before(&self, i: usize) -> Option<TaskId>;  // absorbs the rev().find_map(Entry::id) idiom
```

`replace_entry` / `remove_entry` stay `pub(crate)` and keep their signatures; they become
list+map mutations. `index_of`, `path`, `to_bytes` and `apply` are unchanged — `to_bytes` becomes
"materialise the list in order", `apply` delegates to the `to_loro` mapper from crdt-loro-doc.

`task_before` is the third copy of `entries()[..i].iter().rev().find_map(Entry::id)` — flag it in
`ABSTRACTIONS.md` when the third copy is confirmed, do not extract as a side effect of this task.

## Where the state lives

`DocState` is in `txtodo-daemon`, the Loro doc in `txtodo-crdt`. `allowedDeps` already lets the
daemon depend on `txtodo-crdt` — check `.claude/budgets.json` before writing the `use`. The daemon
keeps owning the actor, the projection write and the expected-write token; `txtodo-crdt` owns only
the document and the mapping. No Loro type appears in a daemon public signature.

## Invariants that must still hold after the swap

- `to_bytes()` is byte-identical to M3 for the same op sequence — the external-edit tests
  (`tasks/daemon-external-edit-tests`) are the goldens and must not be touched.
- `apply` is still all-or-nothing: on `Err` the state is unchanged. Loro transactions give this,
  but the error path must abort the transaction, not commit a half-applied batch.
- `StateError::Unsupported` shrinks (cross-file move and undelete become possible) — narrow it
  deliberately in its own commit, not silently here.
- Blank-line ordering survives a merge: whichever representation crdt-loro-doc picks, a blank run
  between two tasks stays between those two tasks.

## Budgets

Behaviour-preserving refactor and behaviour change never share a commit (CLAUDE.md §4), so this is
at least three: (1) narrow the `DocState` surface, (2) swap the backing store, (3) widen
`Unsupported`. Each ≤ 300 lines. Bench `reconcile_10k_one_edit` must still pass ≤ 20 ms
(`budgets.json.latencyMs`) — run it before and after (2) and record both numbers here.

## Bench (reconcile_10k_one_edit, budget 20 ms)

| when | mean | run |
|---|---|---|
| before the swap (HEAD 46ee551, Vec<Entry>) | 11.5 ms (±4.1) | `cargo bench -p txtodo-daemon --bench reconcile -- --output-format bencher reconcile_10k_one_edit`, 2026-09-12 |

## Swap plan (2026-09-12, awaiting approval before code — constitution §4)

**Finding.** The Loro doc holds each task canonically (LWW prefix fields + a `LoroText`
description). Core's `Quirks` is a `u16` of flags (`TABS`, `TRAILING_WS`, `LEADING_WS`, … carry no
positions), so `txtodo_crdt::rebuild_line` cannot re-emit a quirky line byte-for-byte. A
`to_bytes` materialised from Loro alone rewrites every quirky line on the next write, breaking the
crdt CLAUDE.md invariant "untouched lines materialise byte-identical" and the goldens landed in
47100dc.

**Option A — Loro owns order + merge, raw bytes ride beside it (recommended).**

```rust
pub struct DocState {
    path: FilePath,
    doc: LoroDocument,                 // files/<path> list = order; tasks map = mergeable fields
    raw: BTreeMap<TaskId, OwnedLine>,  // exact bytes per list id (tasks and blank sentinels)
    bom: bool, ending: LineEnding, trailing_newline: bool,
}
pub fn from_file(path: FilePath, file: &File) -> Result<DocState, StateError>; // unchanged
pub fn apply(&mut self, op: &Op) -> Result<(), StateError>;                     // was &OpKind
pub fn to_bytes(&self) -> Vec<u8>;                                              // unchanged
```

- Invariants: every id in the file list has a `raw` line and vice versa (asserted in `to_bytes`);
  `apply` is all-or-nothing (compute the new raw line first, then `txtodo_crdt::apply`; the actor's
  clone-and-swap of `next` already discards a failed batch); one HLC tick per batch still holds.
- Local op: `fields.rs` computes the new bytes exactly as today, then the op goes to Loro. `apply`
  takes `&Op` because LWW registers need the HLC (ADR 0013) — `actor.rs:183` stamps the batch
  before applying instead of after; `history.rs:31` already holds `stored.op`.
- Remote op (sync, later task): apply to Loro, then `raw[task] = rebuild_line(doc, task)` for the
  touched task only. Canonical bytes only where a peer changed something.
- `from_file`: synthetic `Op`s (`Insert` per task, `BlankInsert` per blank, zero HLC, the device
  principal) hydrated through `to_loro::apply`; `raw` filled from the file's lines.
- Files: `state.rs`, `fields.rs`, `actor.rs`, `history.rs`, `state_tests.rs`. Two commits:
  (1) `apply(&OpKind)` → `apply(&Op)` in the callers (behaviour-preserving), (2) the swap.

**Option B — canonical materialisation from Loro alone.** Simpler state, no `raw` map. Rewrites
quirky lines on the next write, fails the goldens, changes M3 bytes. Only if the human drops the
byte-identical invariant for quirky lines.
