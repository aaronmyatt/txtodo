//! Activity-feed DTO (ADR 0004 `oplog.db`, plan M7): mirrors `pb::OpLogEntry`. Split out of
//! `dto.rs`; see that file's module doc.

use serde::Serialize;
use txtodo_proto::v1 as pb;

/// One op-log row (`OpLogStream`), the same source `txtodo blame` reads.
#[derive(Debug, Clone, Serialize)]
pub struct OpLogEntryDto {
    /// `"you@dev"` / `"agent:name@dev"` / `"external@dev"`.
    pub principal: String,
    /// One-line human summary, same shape as `OpSummaryDto::summary`.
    pub op: String,
    /// Unix ms.
    pub at_ms: u64,
    /// Which client made the change: `cli`, `tui`, `desktop`, `mcp`, `sync`, `external`; empty for
    /// an op logged before the daemon kept it (task op-source).
    pub source: String,
}

impl From<pb::OpLogEntry> for OpLogEntryDto {
    fn from(e: pb::OpLogEntry) -> OpLogEntryDto {
        OpLogEntryDto {
            principal: e.principal,
            op: e.op,
            at_ms: e.at_ms,
            source: e.source,
        }
    }
}

/// One op-log row from [`crate::commands_activity::op_log_all`]'s cross-workspace fan-out —
/// `OpLogEntryDto` plus the source workspace, a field `pb::OpLogEntry` itself doesn't carry (task
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
    /// Which client made the change; see `OpLogEntryDto::source`.
    pub source: String,
    /// ULID text of the workspace this entry came from.
    pub workspace_id: String,
    /// That workspace's canonicalized absolute root path.
    pub workspace_root: String,
}
