# Workspace walker discovering todo.txt done.txt notes.md at any depth and on dir create (plan M3)

Plan §3.2.11 (mirrored in `specs/ref-directories.md`): every `todo.txt`, `done.txt`, `notes.md`
under the workspace root, at any depth, is a synced document. Discovery walks the tree; it does not
follow `ref:` tags, so a hand-made directory is picked up too.

## Shape
```rust
pub const DOCUMENT_NAMES: [&str; 3] = ["todo.txt", "done.txt", "notes.md"];
pub fn walk(root: &Path) -> Result<Vec<FilePath>, WalkError>;   // sorted, workspace-relative
```
Plain `std::fs::read_dir` (https://doc.rust-lang.org/std/fs/fn.read_dir.html) with a `Vec<(PathBuf,
depth)>` stack — no recursion (constitution §3), no `walkdir` dependency. `symlink_metadata` decides
whether to descend; symlinked dirs are listed as-is but not entered.

## Bounds
`WALK_MAX_DEPTH = 32` is far above any sane `ref:` nesting; hitting it is a `WalkError::TooDeep`
(validated, not asserted — the tree is external input). `WALK_MAX_FILES = 10_000` matches the
"10 k-line workspace" perf budget language in plan §5.

## Registration
`Workspace::register(file) -> &ActorHandle` is idempotent so the startup walk and later
directory-create rewalks share one path. Removal (file deleted) is out of scope for M3: the actor
stays, its next ExternalChange reads an empty file and reconciles deletes.
