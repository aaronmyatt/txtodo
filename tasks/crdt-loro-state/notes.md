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
