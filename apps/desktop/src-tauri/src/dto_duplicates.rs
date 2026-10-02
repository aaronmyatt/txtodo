//! Duplicate groups (ADR 0032, task sync-drift duplicate-flags): two or more lines of one file
//! that read the same. The daemon derives them from the file when asked (`ListConflicts`'
//! `duplicates`); they are resolved with an ordinary `apply` `delete`, or by editing one copy.
//! `pub` so the e2e bridge (`src/bin/e2e_bridge.rs`) serves the same shape.

use serde::Serialize;
use txtodo_proto::v1 as pb;

/// One copy in a group.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct DuplicateTaskDto {
    /// ULID text.
    pub task_id: String,
    /// 1-based line number, blank lines counted.
    pub line_number: u32,
}

/// One group: its copies, oldest task id first (ULIDs sort by mint time), so the last is the
/// newest.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct DuplicateGroupDto {
    /// The copies.
    pub tasks: Vec<DuplicateTaskDto>,
}

impl From<pb::DuplicateGroup> for DuplicateGroupDto {
    fn from(g: pb::DuplicateGroup) -> DuplicateGroupDto {
        DuplicateGroupDto {
            tasks: g
                .tasks
                .into_iter()
                .map(|t| DuplicateTaskDto {
                    task_id: t.task_id,
                    line_number: t.line_number,
                })
                .collect(),
        }
    }
}

/// A `ListConflicts` reply's groups, as the frontend reads them.
pub fn groups_of(resp: pb::ConflictsResponse) -> Vec<DuplicateGroupDto> {
    resp.duplicates
        .into_iter()
        .map(DuplicateGroupDto::from)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_reply_maps_to_groups_in_order_with_their_copies() {
        let resp = pb::ConflictsResponse {
            flags: Vec::new(),
            duplicates: vec![pb::DuplicateGroup {
                tasks: vec![
                    pb::DuplicateTask {
                        task_id: "01J9K3H5Z7Q8X2M4N6P8R0T2V1".to_owned(),
                        line_number: 2,
                    },
                    pb::DuplicateTask {
                        task_id: "01J9K3H5Z7Q8X2M4N6P8R0T2V2".to_owned(),
                        line_number: 5,
                    },
                ],
            }],
        };
        let groups = groups_of(resp);
        assert_eq!(groups.len(), 1);
        assert_eq!(groups[0].tasks[1].line_number, 5);
        let json = serde_json::to_string(&groups).unwrap_or_default();
        assert!(
            json.contains("\"task_id\":\"01J9K3H5Z7Q8X2M4N6P8R0T2V1\""),
            "{json}"
        );
    }
}
