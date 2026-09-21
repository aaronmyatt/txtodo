//! Pure wire ↔ model conversions for [`crate::grpc_backend::GrpcMcpBackend`]. Split out of
//! `grpc_backend.rs` for the file budget, same pattern as `txtodo-daemon`'s own `convert.rs`.

use txtodo_proto::v1 as pb;

use crate::backend::{FileMeta, Hlc, OpSummary, WorkspaceArg, WorkspaceInfo};

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

/// The folder this server was started in, when that folder is a workspace (task default-workspace):
/// what a call that names no `workspace` means. Set once at startup by `main.rs`; unset, an absent
/// `workspace` stays absent and the daemon answers with its default workspace. A process-wide
/// value because one MCP server is one process with one working folder.
static DEFAULT_WORKSPACE: std::sync::OnceLock<String> = std::sync::OnceLock::new();

/// Makes `path` the workspace a call with no `workspace` arg means. The first call wins.
pub fn set_default_workspace(path: String) {
    let _ = DEFAULT_WORKSPACE.set(path);
}

/// `workspace`, or `default` when the call named none.
fn or_default_workspace(workspace: WorkspaceArg, default: Option<&str>) -> WorkspaceArg {
    workspace.or_else(|| default.map(str::to_owned))
}

/// Turns an MCP-level `workspace` arg into the wire `WorkspaceSelector`: a 26-character Crockford
/// base32 string (a `WorkspaceId` ULID's own encoding) is treated as `workspace_id`, anything else
/// as `path` — mirrors the daemon's own `WorkspaceSelector` oneof (`txtodo.proto`'s doc), sniffed
/// client-side since this crate may not depend on `txtodo_model` to call its real `Ulid::parse`
/// (`budgets.json`'s `allowedDeps`). `None` stays `None` — the daemon's own "sole open workspace"
/// fallback (`workspace_catalog.rs::resolve_sole_open`).
pub fn workspace_selector(workspace: WorkspaceArg) -> Option<pb::WorkspaceSelector> {
    let value = or_default_workspace(workspace, DEFAULT_WORKSPACE.get().map(String::as_str))?;
    let selector = if is_ulid(&value) {
        pb::workspace_selector::Selector::WorkspaceId(value)
    } else {
        pb::workspace_selector::Selector::Path(value)
    };
    Some(pb::WorkspaceSelector {
        selector: Some(selector),
    })
}

/// Crockford base32 (`0-9A-HJKMNP-TV-Z`, case-insensitive, no `I`/`L`/`O`/`U`) at exactly 26
/// characters — a ULID's shape, checked as a format only (this crate never decodes the timestamp/
/// randomness it carries, just tells "id" from "path").
fn is_ulid(s: &str) -> bool {
    s.len() == 26
        && s.bytes().all(|b| {
            let b = b.to_ascii_uppercase();
            b.is_ascii_digit()
                || (b.is_ascii_uppercase() && !matches!(b, b'I' | b'L' | b'O' | b'U'))
        })
}

/// `pb::WorkspaceInfo` into the model's [`WorkspaceInfo`] (`WorkspaceList` RPC).
pub fn workspace_info(w: pb::WorkspaceInfo) -> WorkspaceInfo {
    WorkspaceInfo {
        id: w.workspace_id,
        root: w.root,
        added_at_ms: w.added_at_ms,
        root_exists: w.root_exists,
        has_state: w.has_state,
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

#[cfg(test)]
mod default_workspace_tests {
    use super::*;

    #[test]
    fn a_named_workspace_wins_and_an_absent_one_takes_the_default() {
        assert_eq!(
            or_default_workspace(Some("/named".into()), Some("/here")),
            Some("/named".to_owned())
        );
        assert_eq!(
            or_default_workspace(None, Some("/here")),
            Some("/here".to_owned())
        );
        assert_eq!(
            or_default_workspace(None, None),
            None,
            "no default: left to the daemon"
        );
    }
}
