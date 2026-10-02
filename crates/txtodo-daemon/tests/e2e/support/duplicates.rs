//! Duplicate-group helpers (ADR 0032, `tests/e2e/duplicate_groups.rs`), split out of `mod.rs` for
//! its line budget like `pairing.rs`. A child of `support`, so it reaches `Daemon`'s client.

use super::Daemon;
use txtodo_proto::v1 as pb;

impl Daemon {
    /// `ListConflicts`' duplicate groups for the root `todo.txt`.
    pub async fn duplicate_groups(&mut self) -> Vec<pb::DuplicateGroup> {
        let req = pb::ConflictsRequest {
            path: "todo.txt".to_owned(),
            workspace: None,
        };
        self.client
            .list_conflicts(req)
            .await
            .unwrap_or_else(|e| panic!("list_conflicts: {e}"))
            .into_inner()
            .duplicates
    }

    /// Deletes these tasks of the root `todo.txt` by id, in one `Apply`, leaving no blank.
    pub async fn delete_tasks(&mut self, task_ids: &[String]) -> pb::ApplyResponse {
        let mutations = task_ids
            .iter()
            .map(|id| pb::Mutation {
                kind: Some(pb::mutation::Kind::Delete(pb::Delete {
                    task: Some(pb::TaskRef {
                        line_number: 0,
                        task_id: id.clone(),
                    }),
                    leave_blank: false,
                })),
            })
            .collect();
        let req = pb::ApplyRequest {
            workspace: None,
            path: "todo.txt".to_owned(),
            mutations,
            agent: None,
            source: "test".to_owned(),
            dry_run: false,
        };
        self.client
            .apply(req)
            .await
            .unwrap_or_else(|e| panic!("apply: {e}"))
            .into_inner()
    }
}
