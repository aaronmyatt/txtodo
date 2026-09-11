# SQLite schema migrations/0001.sql — ops, projections, snapshots, meta, WAL (plan M3)

ADR 0004: rusqlite bundled, WAL. Design §4.4: append-only op log at `.txtodo/oplog.db`; everything
under `.txtodo/` is rebuildable from the files, so the store is a cache with history, not the truth.

## Schema
Copy plan M3's DDL verbatim into `crates/txtodo-store/migrations/0001.sql`, then add:
```sql
PRAGMA journal_mode = WAL;     -- https://www.sqlite.org/wal.html
PRAGMA user_version = 1;       -- https://www.sqlite.org/pragma.html#pragma_user_version
```
`payload` is the postcard bytes of `OpKind` (see tasks/model-op-types); `signature` stays NULL until
M4 signs ops. `principal` and `kind` are TEXT tags so `txtodo log` can filter in SQL without decoding.

## API
```rust
pub struct Store { conn: rusqlite::Connection }               // https://docs.rs/rusqlite
pub struct Seq(u64);
impl Store {
    pub fn open(path: &Path) -> Result<Store, StoreError>;
    pub fn append(&mut self, ops: &[Op]) -> Result<Range<Seq>, StoreError>;   // one transaction
    pub fn for_file(&self, file: &FilePath, since: Seq) -> Result<Vec<Op>, StoreError>;
    pub fn between(&self, file: &FilePath, from: &Hlc, to: &Hlc) -> Result<Vec<Op>, StoreError>;
}
```
Append-only is structural: no `UPDATE`/`DELETE` statement exists in the crate; a grep test asserts it.
Every query has an upper bound (`LIMIT` from a named const `MAX_OPS_PER_READ`) — constitution §1.

## Errors
`StoreError { op: &'static str, path, source }` — says what was attempted and on which file.
No `unwrap`; `rusqlite::Error` propagates through `?`. Tests use `tempfile::tempdir()`.
