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
}

impl From<pb::OpLogEntry> for OpLogEntryDto {
    fn from(e: pb::OpLogEntry) -> OpLogEntryDto {
        OpLogEntryDto {
            principal: e.principal,
            op: e.op,
            at_ms: e.at_ms,
        }
    }
}
