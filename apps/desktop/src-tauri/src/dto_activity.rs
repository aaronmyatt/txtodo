//! Activity-feed DTO (ADR 0004 `oplog.db`, plan M7): `pb::OpLogEntry` tagged with its workspace.
//! Split out of `dto.rs`; see that file's module doc.

use serde::Serialize;

/// One op-log row from [`crate::commands_activity::op_log_all`]'s cross-workspace fan-out —
/// a `pb::OpLogEntry` plus the source workspace, a field the entry itself doesn't carry (task
/// `desktop-activity-cross-workspace`: the daemon has nothing to add here, so this is tagged
/// client-side from the `WorkspaceInfo` each entry's fan-out call already came from).
#[derive(Debug, Clone, Serialize)]
pub struct AggregatedOpLogEntryDto {
    /// `"you@dev"` / `"agent:name@dev"` / `"external@dev"`.
    pub principal: String,
    /// One-line human summary, same shape as `OpSummaryDto::summary`.
    pub op: String,
    /// Unix ms.
    pub at_ms: u64,
    /// Which client made the change: `cli`, `tui`, `desktop`, `mcp`, `sync`, `external`; empty for
    /// an op logged before the daemon kept it (task op-source).
    pub source: String,
    /// ULID text of the workspace this entry came from.
    pub workspace_id: String,
    /// That workspace's canonicalized absolute root path.
    pub workspace_root: String,
}
