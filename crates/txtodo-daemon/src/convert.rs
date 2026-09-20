//! The gRPC boundary: proto messages are parsed into typed values once, here (constitution §3:
//! parse, don't validate); ops are rendered into `OpSummary` for log/blame/Watch. Pure.

use crate::handle::{ActorError, ConflictRow, Resolution};
use crate::mutation::{Mutation, MutationError, TaskRef};
use crate::refdir::RefDirError;
use crate::state::TaskCounts;
use tonic::Status;
use txtodo_core::Date;
use txtodo_model::{DeviceId, FilePath, OpKind, Principal, TaskId, TokenId, Ulid};
use txtodo_proto::v1::{self as pb, mutation};
use txtodo_store::{Stored, kind_tag};

/// Line text in a summary is cut here (chars).
pub const SUMMARY_MAX_CHARS: usize = 60;

/// A workspace-relative path from the wire.
pub fn parse_path(s: &str) -> Result<FilePath, Status> {
    FilePath::new(s).map_err(|e| Status::invalid_argument(e.to_string()))
}

/// An actor error onto the wire status it deserves. Lives here, not in `server.rs`, purely to
/// keep that file within its line budget.
pub(crate) fn status_of(e: ActorError) -> Status {
    let details = match &e {
        ActorError::Mutation(m) => Some((m.line(), m.spec_rule())),
        _ => None,
    };
    let mut status = status_kind(e);
    if let Some((line, rule)) = details {
        attach_details(&mut status, line, rule);
    }
    status
}

/// Metadata key carrying the 1-based line a refused mutation is about (task apply-dry-run:
/// structured errors), the same in a dry run and a real apply. `txtodo-mcp` reads the same literal.
pub const ERROR_LINE_KEY: &str = "x-txtodo-error-line";
/// Metadata key carrying the `specs/todotxt.abnf` rule a refusal enforces.
pub const ERROR_RULE_KEY: &str = "x-txtodo-error-rule";

fn attach_details(status: &mut Status, line: Option<u32>, rule: Option<&'static str>) {
    use tonic::metadata::{Ascii, MetadataValue};
    let ascii = |s: &str| s.parse::<MetadataValue<Ascii>>().ok();
    if let Some(v) = line.and_then(|n| ascii(&n.to_string())) {
        status.metadata_mut().insert(ERROR_LINE_KEY, v);
    }
    if let Some(v) = rule.and_then(ascii) {
        status.metadata_mut().insert(ERROR_RULE_KEY, v);
    }
}

fn status_kind(e: ActorError) -> Status {
    match e {
        ActorError::Mutation(MutationError::Stale { .. } | MutationError::StaleBase) => {
            Status::failed_precondition(e.to_string())
        }
        ActorError::Mutation(_) => Status::invalid_argument(e.to_string()),
        ActorError::Unsupported(_) => Status::unimplemented(e.to_string()),
        ActorError::Mirror(_) => Status::internal(e.to_string()),
        ActorError::NoFlag(_) => Status::failed_precondition(e.to_string()),
        ActorError::Gone(_) => Status::unavailable(e.to_string()),
        ActorError::State(_) | ActorError::Store(_) | ActorError::Write(_) | ActorError::Hlc(_) => {
            Status::internal(e.to_string())
        }
        ActorError::RefDir(RefDirError::InvalidSlug(_) | RefDirError::SlugTaken(_)) => {
            Status::invalid_argument(e.to_string())
        }
        ActorError::RefDir(RefDirError::CollisionsExhausted) => {
            Status::resource_exhausted(e.to_string())
        }
        ActorError::RefDir(RefDirError::Io { .. }) => Status::internal(e.to_string()),
        ActorError::Notes(_) => Status::invalid_argument(e.to_string()),
    }
}

/// Classifies a workspace-relative path for `FileInfo.kind` (plan §3.2.5).
pub fn file_kind_of(path: &FilePath) -> pb::FileKind {
    let p = path.to_string();
    if p == "notes.md" || p.ends_with("/notes.md") {
        pb::FileKind::Notes
    } else {
        pb::FileKind::Todo
    }
}

/// `FileInfo.progress` for a TODO-kind file (plan §3.2.5).
pub fn progress_of(todo: TaskCounts) -> pb::Progress {
    pb::Progress {
        done: u32::try_from(todo.completed).unwrap_or(u32::MAX),
        total: u32::try_from(todo.total).unwrap_or(u32::MAX),
    }
}

/// An optional ULID text from the wire (empty = none).
pub fn parse_ulid_opt(s: &str) -> Result<Option<Ulid>, Status> {
    if s.is_empty() {
        return Ok(None);
    }
    Ulid::parse(s)
        .map(Some)
        .ok_or_else(|| Status::invalid_argument(format!("{s:?} is not a ULID")))
}

/// The task id a notes RPC requires: bare `TaskRef` carries no path, so the daemon locates the
/// task by id across the whole workspace (`notes.rs::locate_task`) rather than by line number in
/// a document it does not yet know.
pub(crate) fn parse_required_task_id(t: &pb::TaskRef) -> Result<TaskId, Status> {
    parse_ulid_opt(&t.task_id)?
        .map(TaskId::new)
        .ok_or_else(|| Status::invalid_argument("a task id is required"))
}

pub(crate) fn parse_task_ref(t: Option<pb::TaskRef>) -> Result<TaskRef, Status> {
    let t = t.ok_or_else(|| Status::invalid_argument("mutation needs a task ref"))?;
    let line_number =
        usize::try_from(t.line_number).map_err(|_| Status::invalid_argument("line number"))?;
    if line_number == 0 {
        return Err(Status::invalid_argument("line numbers start at 1"));
    }
    let task_id = parse_ulid_opt(&t.task_id)?.map(TaskId::new);
    Ok(TaskRef {
        line_number,
        task_id,
    })
}

/// A document hash (blake3, 32 bytes) from the wire.
fn parse_hash(bytes: &[u8]) -> Result<[u8; 32], Status> {
    <[u8; 32]>::try_from(bytes).map_err(|_| Status::invalid_argument("base_hash must be 32 bytes"))
}

/// One wire mutation into the typed one.
pub fn parse_mutation(m: pb::Mutation) -> Result<Mutation, Status> {
    let kind = m
        .kind
        .ok_or_else(|| Status::invalid_argument("empty mutation"))?;
    Ok(match kind {
        mutation::Kind::Add(a) => Mutation::Add { line: a.line },
        mutation::Kind::Complete(c) => {
            let today = Date::parse(&c.today)
                .ok_or_else(|| Status::invalid_argument("today must be YYYY-MM-DD"))?;
            Mutation::Complete {
                task: parse_task_ref(c.task)?,
                today,
            }
        }
        mutation::Kind::Edit(e) => Mutation::Edit {
            task: parse_task_ref(e.task)?,
            new_line: e.new_line,
        },
        mutation::Kind::Move(mv) => Mutation::Move {
            task: parse_task_ref(mv.task)?,
            to: parse_path(&mv.to_path)?,
        },
        mutation::Kind::Delete(d) => Mutation::Delete {
            task: parse_task_ref(d.task)?,
            leave_blank: d.leave_blank,
        },
        mutation::Kind::MoveToEnd(m) => Mutation::MoveToEnd {
            task: parse_task_ref(m.task)?,
        },
        mutation::Kind::Reopen(r) => Mutation::Reopen {
            task: parse_task_ref(r.task)?,
        },
        mutation::Kind::MoveBefore(m) => Mutation::MoveBefore {
            task: parse_task_ref(m.task)?,
            before: parse_task_ref(m.before)?,
        },
        mutation::Kind::Replace(r) => Mutation::Replace {
            base: parse_hash(&r.base_hash)?,
            contents: r.contents,
        },
        mutation::Kind::RequireBase(r) => Mutation::RequireBase {
            base: parse_hash(&r.base_hash)?,
        },
    })
}

/// Who is applying: the user on this device unless an agent principal is present (M6).
pub fn parse_principal(
    agent: Option<pb::AgentPrincipal>,
    device: DeviceId,
) -> Result<Principal, Status> {
    let Some(a) = agent else {
        return Ok(Principal::User { device });
    };
    let token =
        parse_ulid_opt(&a.token_id)?.ok_or_else(|| Status::invalid_argument("agent token id"))?;
    if a.name.is_empty() || a.name.len() > 64 {
        return Err(Status::invalid_argument("agent name must be 1..=64 bytes"));
    }
    Ok(Principal::Agent {
        token_id: TokenId::new(token),
        name: a.name,
        device,
    })
}

/// The task an op is about, when it has one.
pub fn task_of(kind: &OpKind) -> Option<TaskId> {
    match kind {
        OpKind::Insert { task, .. }
        | OpKind::SetField { task, .. }
        | OpKind::EditText { task, .. }
        | OpKind::Move { task, .. } => Some(*task),
        OpKind::NotesEdit { .. } | OpKind::BlankInsert { .. } | OpKind::BlankRemove { .. } => None,
    }
}

fn truncate(s: &str) -> String {
    let mut out: String = s.chars().take(SUMMARY_MAX_CHARS).collect();
    if s.chars().count() > SUMMARY_MAX_CHARS {
        out.push('…');
    }
    debug_assert!(out.chars().count() <= SUMMARY_MAX_CHARS + 1);
    out
}

/// One line of human summary per op kind. Never the whole line for edits (privacy of logs is
/// elsewhere; this is the History RPC, which the client asked for).
pub fn summary_of(kind: &OpKind) -> String {
    match kind {
        OpKind::Insert { line, .. } => truncate(line),
        OpKind::SetField { field, value, .. } => format!("{field:?} = {value:?}"),
        OpKind::EditText { edits, .. } => format!("description: {} edit(s)", edits.len()),
        OpKind::Move { after, to_file, .. } => format!("move to {to_file} after {after:?}"),
        OpKind::NotesEdit { edits, .. } => format!("notes: {} edit(s)", edits.len()),
        OpKind::BlankInsert { after } => format!("blank after {after:?}"),
        OpKind::BlankRemove { after } => format!("remove blank after {after:?}"),
    }
}

/// An open flag as the wire sees it.
pub fn to_flag(c: &ConflictRow) -> pb::ReviewFlag {
    pb::ReviewFlag {
        task_id: c.row.task.to_string(),
        line_number: u32::try_from(c.line_number).unwrap_or(u32::MAX),
        mine: String::from_utf8_lossy(&c.row.mine).into_owned(),
        theirs: String::from_utf8_lossy(&c.row.theirs).into_owned(),
        raised_at_ms: c.row.raised_at_ms,
    }
}

/// The wire resolution into the closed enum; unspecified is an error, not a default.
pub fn parse_resolution(raw: i32) -> Result<Resolution, Status> {
    match pb::Resolution::try_from(raw) {
        Ok(pb::Resolution::Mine) => Ok(Resolution::Mine),
        Ok(pb::Resolution::Theirs) => Ok(Resolution::Theirs),
        Ok(pb::Resolution::Merged) => Ok(Resolution::Merged),
        Ok(pb::Resolution::Unspecified) | Err(_) => Err(Status::invalid_argument(
            "resolution must be mine, theirs or merged",
        )),
    }
}

/// A completed mutation/undo/resolve as the wire sees it. Lives here, not `server.rs`, purely to
/// keep that file within its line budget, the same reason `status_of` does.
pub(crate) fn applied_of(a: crate::handle::Applied) -> pb::ApplyResponse {
    pb::ApplyResponse {
        applied: a.applied,
        hash: a.hash.to_vec(),
        hlc_wall_ms: a.hlc.wall_ms,
        hlc_counter: u32::from(a.hlc.counter),
        diff: String::new(),
    }
}

/// A dry run as the wire sees it: no clock stamp, since nothing was ticked, and the diff set.
pub(crate) fn preview_of(p: crate::handle::Preview) -> pb::ApplyResponse {
    pb::ApplyResponse {
        applied: p.applied,
        hash: p.hash.to_vec(),
        hlc_wall_ms: 0,
        hlc_counter: 0,
        diff: p.diff,
    }
}

/// The client name from `ApplyRequest.source`: empty is "not said", anything else is kept as the
/// client wrote it (the store cuts it to `MAX_SOURCE_BYTES`).
pub(crate) fn source_of(source: &str) -> Option<String> {
    (!source.is_empty()).then(|| source.to_owned())
}

/// A stored op as the wire sees it.
pub fn to_summary(s: &Stored) -> pb::OpSummary {
    pb::OpSummary {
        seq: s.seq.0,
        op_id: s.op.id.ulid().to_string(),
        hlc_wall_ms: s.op.hlc.wall_ms,
        hlc_counter: u32::from(s.op.hlc.counter),
        device: s.op.hlc.device.to_string(),
        principal: s.op.principal.to_string(),
        kind: kind_tag(&s.op.kind).to_owned(),
        task_id: task_of(&s.op.kind)
            .map(|t| t.to_string())
            .unwrap_or_default(),
        summary: summary_of(&s.op.kind),
        // Local to this device's log, so not on the op: the caller that has the store fills it.
        source: String::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn progress_of_reports_the_files_own_counts() {
        let todo = TaskCounts {
            total: 3,
            completed: 1,
        };
        let progress = progress_of(todo);
        assert_eq!((progress.done, progress.total), (1, 3));
    }

    #[test]
    fn paths_ulids_and_task_refs_are_validated_once() {
        assert!(parse_path("q4/todo.txt").is_ok());
        assert_eq!(
            parse_path("../x").unwrap_err().code(),
            tonic::Code::InvalidArgument
        );
        assert_eq!(parse_ulid_opt("").unwrap(), None);
        assert!(
            parse_ulid_opt("01ARZ3NDEKTSV4RRFFQ69G5FAV")
                .unwrap()
                .is_some()
        );
        assert!(parse_ulid_opt("nope").is_err());
        let zero = pb::Mutation {
            kind: Some(mutation::Kind::Delete(pb::Delete {
                task: Some(pb::TaskRef {
                    line_number: 0,
                    task_id: String::new(),
                }),
                leave_blank: false,
            })),
        };
        assert!(parse_mutation(zero).is_err());
        let bad_date = pb::Mutation {
            kind: Some(mutation::Kind::Complete(pb::Complete {
                task: Some(pb::TaskRef {
                    line_number: 1,
                    task_id: String::new(),
                }),
                today: "yesterday".into(),
            })),
        };
        assert!(parse_mutation(bad_date).is_err());
        let ok = pb::Mutation {
            kind: Some(mutation::Kind::Add(pb::Add { line: "x".into() })),
        };
        assert_eq!(
            parse_mutation(ok).unwrap(),
            Mutation::Add { line: "x".into() }
        );
    }

    #[test]
    fn principals_and_summaries_render() {
        let device = DeviceId::new(Ulid::from_u128(7));
        assert!(matches!(
            parse_principal(None, device).unwrap(),
            Principal::User { .. }
        ));
        let agent = pb::AgentPrincipal {
            token_id: "01ARZ3NDEKTSV4RRFFQ69G5FAV".into(),
            name: "claude".into(),
        };
        assert!(matches!(
            parse_principal(Some(agent), device).unwrap(),
            Principal::Agent { .. }
        ));
        let long = "x".repeat(100);
        assert_eq!(
            summary_of(&OpKind::Insert {
                task: TaskId::new(Ulid::from_u128(1)),
                after: None,
                line: long
            })
            .chars()
            .count(),
            SUMMARY_MAX_CHARS + 1
        );
        assert_eq!(
            summary_of(&OpKind::BlankInsert { after: None }),
            "blank after None"
        );
    }
}
