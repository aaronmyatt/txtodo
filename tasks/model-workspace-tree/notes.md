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

## As built (2026-09-13, agent)

`crates/txtodo-model/src/tree.rs` (+ `tree_tests.rs`), pure and no-I/O as required:

- `NodeId(Option<FilePath>)`: the workspace root (`None`) or a `ref:` directory, reusing
  `FilePath`'s validation since a directory path has exactly the same shape as a document path.
  `NodeId::of_file(file)` is the dirname-or-root projection every other piece uses.
- `WorkspaceTree::build(Vec<NodeInput>)`: `NodeInput` is `{id, progress: Progress{done,total},
  ref_tags: Vec<RefTag{owner, slug}>}`, supplied by the caller — this crate never walks a
  filesystem or parses a line. A tag whose target directory isn't among the inputs is a dangling
  edge (rule 9) and produces no child; a directory with no owner is a legitimate node
  (`WorkspaceTree::orphans`, rule 10). `MAX_TREE_DEPTH` (32) and `MAX_TRACKED_REFS` (10 000) are
  asserted bounds, matching `txtodo-daemon::walker`'s own document caps.
- The "not a general graph" claim in the module doc is proven, not just asserted: since a node's
  parent is always its own dirname, a child's path is always strictly longer than its parent's, so
  a cycle is structurally impossible. A proptest (`children_are_always_deeper_than_their_parent`)
  fuzzes random slug sets through `build` and checks exactly that.
- `invalidates(op: &OpKind, file: &FilePath) -> Invalidation` (`{counters: Vec<NodeId>, edges:
  Vec<NodeId>}`), exhaustive over `OpKind`, no default arm. One deliberate widening beyond this
  file's first draft: `Insert` invalidates edges too, not just counters — a freshly inserted line
  (a plain `add "task ref:foo"`, or a cross-file `Move`'s destination side, which is itself
  recorded as an `Insert`) can carry a hand-typed `ref:` tag from the moment it lands, so treating
  Insert as counters-only would miss a same-tick edge change. `SetField{Deleted}` is likewise
  counters+edges (removing a line can remove the `ref:` tag it carried, orphaning its target).
- 14 unit tests + 1 proptest, all green (`cargo test -p txtodo-model`).

## Gap for the human to double check

The daemon's integration of this tree (see tasks/proto-tree-progress/notes.md's As-built) caches a
*rebuild* of the tree behind a dirty flag rather than patching individual nodes in place per
`invalidates`'s per-node output. `invalidates` itself is still exactly as specified (precise,
per-node, exhaustive) and is used to decide *whether* a rebuild is worth doing at all — the
daemon-side simplification is about *how* the rebuild happens, not about this module's contract.
Flagging here since "cached and invalidated on ops" could be read as asking for true incremental
per-node patching; see that task's notes for the tradeoff this session made instead.
