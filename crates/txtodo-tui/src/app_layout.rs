//! `app.rs`'s layout half (task layout-hot-reload-clients): the daemon announces a layout change
//! as a `Change` for `txtodo.toml` on every `Watch` stream; the TUI then re-reads the root list
//! and, when it is a different document, switches to it instead of keeping the one it opened at
//! startup. Its own file only for `app.rs`'s line budget.

use txtodo_proto::v1 as pb;

use crate::app::rebaseline;
use crate::daemon::{Daemon, DaemonError};
use crate::state::AppState;

/// The `Change.path` the daemon uses for "the layout changed" (`txtodo-daemon`'s
/// `watch_forward::LAYOUT_CHANGE_PATH`; this crate may not depend on that crate).
pub const LAYOUT_CHANGE_PATH: &str = "txtodo.toml";

/// Whether `change` is the layout notice rather than a document change.
pub fn is_layout_change(change: &pb::Change) -> bool {
    change.path == LAYOUT_CHANGE_PATH
}

/// Re-reads the root list; when it moved, points `state` at the new document, re-baselines it
/// and returns the `Watch` stream to replace the old one with. `None` when nothing moved.
pub async fn follow_root_list(
    daemon: &mut Daemon,
    state: &mut AppState,
) -> Result<Option<tonic::Streaming<pb::Change>>, DaemonError> {
    let path = daemon.root_list().await?;
    if path == state.path {
        return Ok(None);
    }
    state.path = path;
    let watch = daemon.watch(vec![state.path.clone()]).await?;
    let file = daemon.get_file(&state.path).await?;
    rebaseline(state, &file);
    Ok(Some(watch))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_the_layout_path_is_a_layout_change() {
        let layout = pb::Change {
            path: LAYOUT_CHANGE_PATH.to_owned(),
            ..pb::Change::default()
        };
        let doc = pb::Change {
            path: "todo.txt".to_owned(),
            ..pb::Change::default()
        };
        assert!(is_layout_change(&layout));
        assert!(!is_layout_change(&doc));
    }
}
