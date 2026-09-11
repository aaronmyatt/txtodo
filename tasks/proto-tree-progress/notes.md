# gRPC `ListFiles` returns the tree with progress; `Watch` emits progress changes — M5

Plan M5, specs/ref-directories.md rule 5. The tree and the counters are owned by
[model-workspace-tree](../model-workspace-tree/notes.md); the proto layer is a projection, not a
second source of truth.

## ListFiles: flat list → tree

M3 `ListFiles` returns a flat file list. M5 adds parent/child edges (from `ref:` tags plus
directory layout) and a `Progress { done, total }` per node, computed exactly as rule 5 says:
non-recursive, `done = completed lines in <ref>/todo.txt + task lines in <ref>/done.txt`, blanks
excluded. Reuse the tree model's `progress(ref)`; do not recompute in the proto layer.

## Watch: push the invalidation, not a re-list

When an op invalidates a ref's counters, `Watch` emits a progress change for that ref. The
`invalidates(op, file)` function in `model-workspace-tree` is the single source of "which refs
changed"; the Watch stream turns each invalidation into one message. A full re-list per keystroke
is the expensive wrong version — the cache exists precisely to avoid it.

## Proto discipline

New messages/fields in `crates/txtodo-proto/proto/txtodo/v1/txtodo.proto`, **new field numbers
only** — never change or reuse an existing number (wire compat). Regen is checked by
`tasks/proto-grpc`'s regen step; keep it green.

## The `n/m` indicator is a client concern

Rule 5 displays `open/total` on the parent line's indicator and `done of total` in the detail
header. The proto only carries `done`/`total` per node; how the CLI/desktop renders it is not this
task. Keep the field names neutral (`done`, `total`) so the renderer can choose.

## Tests

- `ListFiles` on the 3-level fixture returns the tree with rule-5 progress, including lines in
  `done.txt`, and no recursion in the counter.
- `Watch` emits a progress change when a completion lands in `<ref>/todo.txt`, and nothing for an
  `EditText` that changes no `ref:` edge.
- Regen check green; new field numbers are additive.
