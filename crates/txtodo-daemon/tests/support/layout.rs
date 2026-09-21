//! Ref-dir helpers for the layout tests (task workspace-layout). A child module, so it reads
//! `Daemon`'s private client.

use super::Daemon;
use txtodo_proto::v1::{self as pb};

impl Daemon {
    /// `RefDir` on the sole open workspace's `todo.txt`, line `line`, creating the directory when
    /// `ensure`.
    pub async fn ref_dir(&mut self, line: u32, ensure: bool) -> pb::RefDirInfo {
        self.ref_dir_of("todo.txt", line, ensure).await
    }

    /// `ref_dir` for the document at `path`, which is the root list when the layout says so.
    pub async fn ref_dir_of(&mut self, path: &str, line: u32, ensure: bool) -> pb::RefDirInfo {
        let req = pb::RefDirRequest {
            path: path.into(),
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

    /// The bytes the daemon holds for `path` (a document that is not registered is an error).
    pub async fn bytes_of(&mut self, path: &str) -> Vec<u8> {
        self.client
            .get_file(pb::GetFileRequest {
                path: path.into(),
                workspace: None,
            })
            .await
            .unwrap_or_else(|e| panic!("get_file {path}: {e}"))
            .into_inner()
            .bytes
    }

    /// The paths of every document the daemon has registered.
    pub async fn documents(&mut self) -> Vec<String> {
        self.client
            .list_files(pb::ListFilesRequest { workspace: None })
            .await
            .unwrap_or_else(|e| panic!("list_files: {e}"))
            .into_inner()
            .files
            .into_iter()
            .map(|f| f.path)
            .collect()
    }
}
