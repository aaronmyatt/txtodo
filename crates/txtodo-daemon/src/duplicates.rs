//! Exact-duplicate lines as a conflict the user resolves, derived from the file (ADR 0032,
//! `tasks/sync-drift/duplicate-flags/`). Nothing is stored: every call builds the groups from the
//! document as it is now, so a group goes away as soon as the file no longer has it, and paired
//! devices that hold the same file agree on the same groups.

use std::borrow::Cow;
use std::collections::HashMap;

use crate::id_strip::without_own_id;
use crate::state::{DocState, Entry};
use txtodo_core::OwnedLine;
use txtodo_model::{IdentityMode, TaskId};

/// Two or more tasks in one file whose whole lines are the same bytes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DuplicateGroup {
    /// Each task and its 1-based line number (blank lines counted), oldest id first: ids are
    /// ULIDs, which sort by the time they were minted (<https://github.com/ulid/spec>).
    pub tasks: Vec<(TaskId, usize)>,
}

/// Every duplicate group in `state`, in the order of each group's first line. Blank lines never
/// count; done lines do; the same line in another file is another document's business.
pub fn duplicate_groups(state: &DocState) -> Vec<DuplicateGroup> {
    let mut by_line: HashMap<Cow<'_, [u8]>, Vec<(TaskId, usize)>> = HashMap::new();
    for (i, entry) in state.indexed_entries() {
        let Entry::Task { id, line } = entry else {
            continue;
        };
        by_line
            .entry(comparable(state.mode(), line))
            .or_default()
            .push((*id, i + 1));
    }
    let mut groups: Vec<DuplicateGroup> = by_line
        .into_values()
        .filter(|tasks| tasks.len() > 1)
        .map(|mut tasks| {
            tasks.sort_by_key(|(id, _)| *id);
            DuplicateGroup { tasks }
        })
        .collect();
    groups.sort_by_key(|g| g.tasks.iter().map(|(_, n)| *n).min());
    debug_assert!(groups.iter().all(|g| g.tasks.len() > 1));
    groups
}

/// A group on the wire (`ConflictsResponse.duplicates`).
pub(crate) fn to_duplicate_group(group: &DuplicateGroup) -> txtodo_proto::v1::DuplicateGroup {
    txtodo_proto::v1::DuplicateGroup {
        tasks: group
            .tasks
            .iter()
            .map(|(task, line)| txtodo_proto::v1::DuplicateTask {
                task_id: task.to_string(),
                line_number: u32::try_from(*line).unwrap_or(u32::MAX),
            })
            .collect(),
    }
}

/// The bytes two lines are compared by. In tagged mode each line carries its own `id:` tag, so no
/// two would ever match; that tag is identity, not text, and is left out. Everything else counts.
fn comparable(mode: IdentityMode, line: &OwnedLine) -> Cow<'_, [u8]> {
    match mode {
        IdentityMode::Tagged => match without_own_id(line) {
            Some(stripped) => Cow::Owned(stripped),
            None => Cow::Borrowed(line.bytes()),
        },
        IdentityMode::Sidecar => Cow::Borrowed(line.bytes()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::task_id;
    use txtodo_core::parse_file;
    use txtodo_model::FilePath;

    /// `lines` as a file: `(id, text)`, an empty text a blank line (its id unused).
    fn doc(mode: IdentityMode, lines: &[(u128, &str)]) -> DocState {
        let path = FilePath::new("todo.txt").unwrap_or_else(|e| panic!("{e}"));
        let text: String = lines.iter().map(|(_, l)| format!("{l}\n")).collect();
        let file = parse_file(text.as_bytes());
        let ids: Vec<Option<TaskId>> = lines
            .iter()
            .map(|(n, l)| (!l.is_empty()).then(|| task_id(*n)))
            .collect();
        DocState::from_file(path, &file, &ids, mode).unwrap_or_else(|e| panic!("{e}"))
    }

    fn groups(mode: IdentityMode, lines: &[(u128, &str)]) -> Vec<Vec<(u128, usize)>> {
        duplicate_groups(&doc(mode, lines))
            .into_iter()
            .map(|g| {
                g.tasks
                    .into_iter()
                    .map(|(id, n)| (id.ulid().to_u128(), n))
                    .collect()
            })
            .collect()
    }

    /// A table row: its name, the file's lines, the groups it must give.
    type Case<'a> = (&'a str, &'a [(u128, &'a str)], Vec<Vec<(u128, usize)>>);

    #[test]
    fn table() {
        let s = IdentityMode::Sidecar;
        let cases: &[Case] = &[
            ("no repeats", &[(1, "a"), (2, "b")], vec![]),
            (
                "two copies, oldest id first",
                &[(9, "a"), (3, "a")],
                vec![vec![(3, 2), (9, 1)]],
            ),
            (
                "three copies",
                &[(1, "a"), (2, "a"), (3, "a")],
                vec![vec![(1, 1), (2, 2), (3, 3)]],
            ),
            (
                "blank lines never",
                &[(1, "a"), (0, ""), (0, ""), (2, "b")],
                vec![],
            ),
            (
                "line numbers count blanks",
                &[(1, "a"), (0, ""), (2, "a")],
                vec![vec![(1, 1), (2, 3)]],
            ),
            (
                "done lines count",
                &[(1, "x 2026-09-27 a"), (2, "x 2026-09-27 a")],
                vec![vec![(1, 1), (2, 2)]],
            ),
            (
                "a priority makes them differ",
                &[(1, "(A) a"), (2, "a")],
                vec![],
            ),
            (
                "groups in first-line order",
                &[(1, "b"), (2, "a"), (3, "b"), (4, "a")],
                vec![vec![(1, 1), (3, 3)], vec![(2, 2), (4, 4)]],
            ),
        ];
        for (name, lines, want) in cases {
            assert_eq!(&groups(s, lines), want, "{name}");
        }
    }

    #[test]
    fn in_tagged_mode_the_own_id_tag_is_not_text() {
        let a = "01M2D3AAAAAAAAAAAAAAAAAAAA";
        let b = "01M2D3BBBBBBBBBBBBBBBBBBBB";
        let ida = txtodo_model::Ulid::parse(a)
            .unwrap_or_else(|| panic!("ulid"))
            .to_u128();
        let idb = txtodo_model::Ulid::parse(b)
            .unwrap_or_else(|| panic!("ulid"))
            .to_u128();
        let lines = [
            (ida, format!("buy milk id:{a}")),
            (idb, format!("buy milk id:{b}")),
        ];
        let lines: Vec<(u128, &str)> = lines.iter().map(|(n, l)| (*n, l.as_str())).collect();
        assert_eq!(
            groups(IdentityMode::Tagged, &lines),
            vec![vec![(ida, 1), (idb, 2)]]
        );
    }
}
