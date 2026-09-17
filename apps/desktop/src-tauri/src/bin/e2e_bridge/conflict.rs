//! The `debug_raise_conflict` bridge command, split out of `e2e_bridge.rs` for its own
//! file-length budget — same pattern `workspace.rs`/`activity.rs` use.

use serde::Deserialize;
use serde_json::Value;
use txtodo_model::{FilePath, TaskId, Ulid};
use txtodo_store::{ReviewRow, Store};

use super::{ApiError, parse};

/// Raises a `needs_review` flag directly in `.txtodo/oplog.db`, the same way
/// `crates/txtodo-daemon/tests/grpc.rs::raise_flag` does for the daemon's own tests: "what an
/// import merge would do... no actual sync is needed." Real daemon-to-daemon sync has no
/// transport wired up yet at all (`crates/txtodo-daemon/src/pairing_grpc.rs`'s own doc comment;
/// see `todo.txt`'s `sync-loopback-converge` entry) — this is not a workaround invented for this
/// harness, it's the same substitute the daemon team already uses to test `ListConflicts`/
/// `ResolveConflict` without it. WAL mode lets this connection share the file safely with the
/// live daemon's own connection to the same database.
pub(crate) fn cmd_debug_raise_conflict(
    workspace: &std::path::Path,
    args: Value,
) -> Result<Value, ApiError> {
    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct Req {
        path: String,
        task_id: String,
        mine: String,
        theirs: String,
    }
    let r: Req = parse(args)?;
    let file = FilePath::new(&r.path)
        .map_err(|e| ApiError(format!("e2e_bridge: invalid path {:?}: {e}", r.path)))?;
    let ulid = Ulid::parse(&r.task_id)
        .ok_or_else(|| ApiError(format!("e2e_bridge: invalid task_id {:?}", r.task_id)))?;
    let raised_at_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(1);

    let mut store = Store::open(&workspace.join(".txtodo").join("oplog.db"))
        .map_err(|e| ApiError(format!("e2e_bridge: Store::open: {e}")))?;
    store
        .raise_flag(&ReviewRow {
            file,
            task: TaskId::new(ulid),
            raised_at_ms,
            mine: r.mine.into_bytes(),
            theirs: r.theirs.into_bytes(),
        })
        .map_err(|e| ApiError(format!("e2e_bridge: raise_flag: {e}")))?;
    Ok(Value::Null)
}
