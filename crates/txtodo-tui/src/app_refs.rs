//! The Tasks rows' `ref:` badges (task `tui-revamp/tui-tasks`), read from the daemon's file tree
//! (`ListFiles`): each child of the open document's node is one `ref:` directory, named by its
//! slug, with its own done/total. Refreshed at startup and on the 1 s tick, since a change to a
//! sub-list comes on no `Watch` stream the TUI holds.

use std::collections::BTreeMap;

use txtodo_proto::v1 as pb;

use crate::daemon::Daemon;
use crate::state::AppState;
use crate::state_tasks::RefBadge;

/// Re-reads the badges; a failed read keeps the old ones.
pub async fn refresh(daemon: &mut Daemon, state: &mut AppState) {
    if let Ok(files) = daemon.list_files().await
        && let Some(tree) = files.tree
    {
        state.tasks.refs = badges(&tree, &state.path);
    }
}

/// The badges for the document at `path`: its node's children, by slug. A directory with a
/// `todo.txt` of task lines shows its progress; any other shows the notes mark. The tree holds
/// only directories that exist, and lists a `notes.md` only once the daemon has opened it, so "a
/// directory without tasks" is the closest reading of "has notes" it offers.
pub fn badges(tree: &pb::TreeNode, path: &str) -> BTreeMap<String, RefBadge> {
    let Some(node) = find(tree, path) else {
        return BTreeMap::new();
    };
    node.children
        .iter()
        .filter_map(|child| {
            let slug = child.dir.rsplit('/').next()?.to_owned();
            let progress = child.progress.unwrap_or_default();
            let badge = if progress.total > 0 {
                RefBadge::Progress {
                    done: progress.done,
                    total: progress.total,
                }
            } else {
                RefBadge::Notes
            };
            Some((slug, badge))
        })
        .collect()
}

/// The node whose own files include `path`.
fn find<'t>(node: &'t pb::TreeNode, path: &str) -> Option<&'t pb::TreeNode> {
    if node.files.iter().any(|f| f.path == path) {
        return Some(node);
    }
    node.children.iter().find_map(|c| find(c, path))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn file(path: &str) -> pb::FileInfo {
        pb::FileInfo {
            path: path.to_owned(),
            ..pb::FileInfo::default()
        }
    }

    fn node(
        dir: &str,
        files: &[&str],
        done: u32,
        total: u32,
        children: Vec<pb::TreeNode>,
    ) -> pb::TreeNode {
        pb::TreeNode {
            dir: dir.to_owned(),
            progress: Some(pb::Progress { done, total }),
            owner_task_id: String::new(),
            files: files.iter().map(|p| file(p)).collect(),
            children,
        }
    }

    #[test]
    fn badges_come_from_the_open_documents_children() {
        let tree = node(
            "",
            &["todo.txt"],
            1,
            4,
            vec![
                node(
                    "tasks/trip",
                    &["tasks/trip/todo.txt"],
                    2,
                    5,
                    vec![node(
                        "tasks/trip/tickets",
                        &["tasks/trip/tickets/notes.md"],
                        0,
                        0,
                        vec![],
                    )],
                ),
                node("tasks/book", &[], 0, 0, vec![]),
            ],
        );
        let root = badges(&tree, "todo.txt");
        assert_eq!(
            root.get("trip"),
            Some(&RefBadge::Progress { done: 2, total: 5 })
        );
        assert_eq!(root.get("book"), Some(&RefBadge::Notes), "no tasks: notes");
        assert_eq!(root.len(), 2, "grandchildren belong to the sub-list");
        let sub = badges(&tree, "tasks/trip/todo.txt");
        assert_eq!(sub.get("tickets"), Some(&RefBadge::Notes));
    }
}
