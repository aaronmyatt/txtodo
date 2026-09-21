//! Ref-dir helpers for the layout tests (task workspace-layout). A child module, so it reads
//! `Daemon`'s private client.

use super::Daemon;
use txtodo_proto::v1::{self as pb};

impl Daemon {
    /// `RefDir` on the sole open workspace's `todo.txt`, line `line`, creating the directory when
    /// `ensure`.
    pub async fn ref_dir(&mut self, line: u32, ensure: bool) -> pb::RefDirInfo {
        let req = pb::RefDirRequest {
            path: "todo.txt".into(),
            task: Some(pb::TaskRef {
                line_number: line,
                task_id: String::new(),
            }),
            ensure,
            workspace: None,
        };
        self.client
            .ref_dir(req)
            .await
            .unwrap_or_else(|e| panic!("ref_dir: {e}"))
            .into_inner()
    }
}
