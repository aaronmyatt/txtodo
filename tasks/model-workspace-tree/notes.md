# Workspace tree model with progress computation, cached and invalidated on ops (plan M5)

`specs/ref-directories.md` rule 5 is the whole specification:

> `done = completed lines in <ref>/todo.txt + task lines in <ref>/done.txt`;
> `total = task lines in <ref>/todo.txt + task lines in <ref>/done.txt`; blank lines excluded.

Note what it does **not** say: progress does not recurse. A parent's counter reflects its immediate
sub-list, not the whole subtree. Rule 5 is normative and the spec is mirror-checked, so if recursive
progress is wanted that is a spec change with the human, not a reading of it. Write the
non-recursive rule into the doc comment with a pointer at rule 5, because "surely it should sum the
grandchildren" is the bug someone will helpfully introduce.

## Tree shape

Discovery is by **walking**, not by following tags (rule 11), and `tasks/daemon-workspace-walker`
already does that at M3. So the tree model does not discover anything; it is given the file set and
builds the parent/child edges from `ref:` tags plus directory layout (rule 2: the slug names a
directory beside the file that holds the line).

Consequences worth asserting rather than assuming:

- A directory with no line pointing at it is a legitimate node — `prune --orphans` exists precisely
  because those happen (rule 10). The tree holds it; it just has no parent edge.
- A `ref:` with no directory is legitimate too (rule 9, dangling). The edge exists, the node does
  not.
- Therefore the structure is **not** a tree in the strict sense until both are handled. Model it as
  a directed graph over directories and assert acyclicity explicitly — a hand-made symlink or a
  hand-edited tag can produce a cycle, and every consumer (progress, the desktop breadcrumb,
  `prune`) will hang on it. `MAX_TREE_DEPTH`, asserted, is the second line of defence.

## Caching, and the only hard part

"Cached and invalidated on ops" is where this goes wrong. The cheap wrong version recomputes the
whole workspace on every op; the expensive wrong version caches and misses an invalidation.

Make invalidation derivable from the op, not from a hook someone remembers to call:

```rust
/// Which cached counters an op can possibly change. Exhaustive over OpKind.
fn invalidates(op: &OpKind, file: &FilePath) -> Invalidation
```

- `SetField{field: Completed, ..}` on `<ref>/todo.txt` → that ref's counters only.
- `Insert` / `Move` / `SetField{Deleted}` → the source file's ref, and for a cross-file `Move` the
  destination's too.
- `EditText` on a description that gains or loses a `ref:` tag → an **edge** change, not a counter
  change. This is the one that gets missed: the text edit looks like it touches nothing structural.
- `NotesEdit`, `BlankInsert`, `BlankRemove` → nothing. Blanks are excluded by rule 5, so say so in
  the match arm rather than leaving it to a default.

Exhaustive match, no default arm (CLAUDE.md §3), so a new `OpKind` forces a decision here.

## Verifying the cache instead of trusting it

Cache bugs are silent. Add a debug-only `recompute_and_assert_equal` that, under `debug_assertions`,
recomputes counters from scratch after each invalidation batch and asserts equality with the cached
value. It costs nothing in release and turns every missed invalidation in dev and test into a crash
— which is what CLAUDE.md §3 asks of an invariant.

Pair it with a property test: for any op sequence, cached counters equal freshly computed ones.
That is the invariant; the individual arm tests are just faster feedback.

## Budgets

Counters are `u32` with an asserted cap; a file over `MAX_LINES_PER_FILE` is already refused by
`DocState`. The cache is a bounded map keyed by ref directory — `MAX_TRACKED_REFS`, asserted,
because "one entry per directory in the workspace" is an unbounded collection unless someone says
otherwise.
