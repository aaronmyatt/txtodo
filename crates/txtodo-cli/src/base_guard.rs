//! A batch that addresses an existing line by number alone can't tell a shifted line from the one
//! the command read: sidecar text carries no `id:`, so `TaskRef.task_id` is empty and the daemon
//! has nothing to compare (in tagged mode a stale line is refused as `Stale` per mutation). Such a
//! batch leads with a `RequireBase` naming the hash the command read, and the daemon refuses the
//! whole batch if the document has moved on since. A batch whose every addressed line carries an
//! id checks itself, and one that only appends addresses nothing, so neither needs the guard (and
//! neither is refused for an unrelated concurrent change).

use txtodo_proto::v1::{self as pb, mutation};

/// The line a mutation addresses, if it addresses an existing one. Exhaustive on purpose: a new
/// mutation kind has to decide here whether it needs the guard.
fn addressed(kind: &mutation::Kind) -> Option<&pb::TaskRef> {
    match kind {
        mutation::Kind::Complete(m) => m.task.as_ref(),
        mutation::Kind::Edit(m) => m.task.as_ref(),
        mutation::Kind::Move(m) => m.task.as_ref(),
        mutation::Kind::Delete(m) => m.task.as_ref(),
        mutation::Kind::MoveToEnd(m) => m.task.as_ref(),
        // Two lines: report one that lacks an id if either does, since that is the one the guard
        // exists for.
        mutation::Kind::MoveBefore(m) => [m.task.as_ref(), m.before.as_ref()]
            .into_iter()
            .flatten()
            .find(|t| t.task_id.is_empty())
            .or(m.task.as_ref()),
        mutation::Kind::Add(_) | mutation::Kind::Replace(_) | mutation::Kind::RequireBase(_) => {
            None
        }
    }
}

/// `mutations`, led by a `RequireBase` on `base_hash` (what `Daemon::snapshot` returned for the
/// bytes the command started from) when any of them addresses a line by number alone.
pub fn guarded(base_hash: &[u8], mut mutations: Vec<pb::Mutation>) -> Vec<pb::Mutation> {
    debug_assert!(
        !base_hash.is_empty(),
        "only a document the daemon knows is diffed"
    );
    let by_number_alone = mutations
        .iter()
        .filter_map(|m| m.kind.as_ref())
        .filter_map(addressed)
        .any(|task| task.task_id.is_empty());
    if by_number_alone {
        let guard = pb::RequireBase {
            base_hash: base_hash.to_vec(),
        };
        mutations.insert(
            0,
            pb::Mutation {
                kind: Some(mutation::Kind::RequireBase(guard)),
            },
        );
    }
    mutations
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::daemon_mode::plan_mutations;
    use txtodo_core::parse_file;

    const HASH: [u8; 32] = [7; 32];
    const ID: &str = "01ARZ3NDEKTSV4RRFFQ69G5FAA";

    fn plan(old: &str, new: &str) -> Vec<pb::Mutation> {
        let (old, new) = (parse_file(old.as_bytes()), parse_file(new.as_bytes()));
        plan_mutations(&old, &new).expect("expressible")
    }

    fn leads_with_guard(muts: &[pb::Mutation]) -> bool {
        match muts.first().and_then(|m| m.kind.as_ref()) {
            Some(mutation::Kind::RequireBase(g)) => g.base_hash == HASH,
            _ => false,
        }
    }

    #[test]
    fn a_line_number_only_delete_or_edit_is_led_by_the_guard() {
        // Sidecar text: no `id:`, so a delete can only name its line by number.
        let del = plan("a\nb\nc\n", "b\nc\n");
        assert_eq!(del.len(), 1);
        let guarded = guarded(&HASH, del.clone());
        assert!(leads_with_guard(&guarded), "{guarded:?}");
        assert_eq!(
            &guarded[1..],
            &del[..],
            "the batch itself is untouched, in order"
        );
    }

    #[test]
    fn a_batch_whose_lines_carry_ids_or_that_only_appends_is_not() {
        let tagged = format!("a id:{ID}\nb\n");
        let edit = plan(&tagged, &format!("(A) a id:{ID}\nb\n"));
        assert!(
            !leads_with_guard(&guarded(&HASH, edit)),
            "the id checks the line itself"
        );
        let append = plan("a\n", "a\nb\n");
        assert!(
            !leads_with_guard(&guarded(&HASH, append)),
            "an append addresses no line"
        );
    }

    #[test]
    fn one_line_without_an_id_is_enough() {
        let old = format!("a id:{ID}\nb\nc\n");
        let new = format!("a id:{ID}\nc\n");
        assert!(
            leads_with_guard(&guarded(&HASH, plan(&old, &new))),
            "`b` has no id to check"
        );
    }
}
