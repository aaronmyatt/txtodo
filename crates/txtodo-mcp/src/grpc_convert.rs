//! Pure wire ↔ model conversions for [`crate::grpc_backend::GrpcMcpBackend`]. Split out of
//! `grpc_backend.rs` for the file budget, same pattern as `txtodo-daemon`'s own `convert.rs`.

use txtodo_proto::v1 as pb;

use crate::backend::{FileMeta, Hlc, OpSummary};

/// Lowercase hex, no `0x` prefix (the daemon's `blake3` projection hashes and op ids are shown
/// this way everywhere else in the CLI/daemon).
pub fn hex(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        out.push_str(&format!("{b:02x}"));
    }
    out
}

/// `(wall_ms, counter)` into the model's [`Hlc`].
pub fn hlc(wall_ms: u64, counter: u32) -> Hlc {
    Hlc { wall_ms, counter }
}

/// `pb::FileInfo` into [`FileMeta`]; an unrecognised/unspecified kind is dropped by the caller.
pub fn file_meta(info: &pb::FileInfo) -> Option<FileMeta> {
    let kind = match pb::FileKind::try_from(info.kind).ok()? {
        pb::FileKind::Todo => "todo",
        pb::FileKind::Done => "done",
        pb::FileKind::Notes => "notes",
        pb::FileKind::Unspecified => return None,
    };
    Some(FileMeta {
        path: info.path.clone(),
        kind,
    })
}

/// `pb::OpSummary` into the model's [`OpSummary`].
pub fn op_summary(o: pb::OpSummary) -> OpSummary {
    OpSummary {
        seq: o.seq,
        op_id: o.op_id,
        hlc: hlc(o.hlc_wall_ms, o.hlc_counter),
        device: o.device,
        principal: o.principal,
        kind: o.kind,
        task_id: (!o.task_id.is_empty()).then_some(o.task_id),
        summary: o.summary,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hex_matches_lowercase_no_prefix() {
        assert_eq!(hex(&[0x0a, 0xff]), "0aff");
        assert_eq!(hex(&[]), "");
    }

    #[test]
    fn file_meta_maps_known_kinds_and_drops_unspecified() {
        let mut info = pb::FileInfo {
            path: "todo.txt".to_owned(),
            hash: vec![],
            kind: pb::FileKind::Todo as i32,
            progress: None,
        };
        assert_eq!(file_meta(&info).map(|m| m.kind), Some("todo"));
        info.kind = pb::FileKind::Unspecified as i32;
        assert!(file_meta(&info).is_none());
    }
}
