//! `Host`: the browser's `txtodod`. One store, this device's id, the open lists, and the change
//! events waiting for the page. `call` answers one frontend command (`apps/desktop/src/lib/
//! daemon.ts`'s names and arguments) with its JSON result; commands about the app, the window or
//! other devices are the Worker shim's, not this host's.

use std::collections::BTreeMap;

use serde::Deserialize;
use serde::de::DeserializeOwned;
use serde_json::Value;
use txtodo_engine::duplicates::duplicate_groups;
use txtodo_engine::local_date::today_local;
use txtodo_model::{DeviceId, FilePath, Principal, Ulid};
use txtodo_store::Store;

use crate::clock::{new_ulid, now_ms};
use crate::doc::{By, Doc, all_ops};
use crate::dto::{
    ApplyResultDto, ChangeDto, DuplicateGroupDto, FileContentsDto, FileInfoDto, HistoryDto,
    OpSummaryDto, hex, task_of,
};
use crate::error::WebError;
use crate::mutation_dto::MutationDto;

/// The `meta` key holding this device's id (16 bytes, big-endian ULID).
pub const DEVICE_KEY: &str = "web/device";
/// The `meta` key holding the known lists, one path per line.
pub const FILES_KEY: &str = "web/files";
/// The list a new workspace starts with.
pub const ROOT_LIST: &str = "todo.txt";
/// Most change events kept for the page; the oldest go first past it.
pub const MAX_PENDING_CHANGES: usize = 256;
/// Most lists one workspace holds here.
pub const MAX_LISTS: usize = 1_000;

/// The browser's daemon.
pub struct Host {
    store: Store,
    device: DeviceId,
    files: Vec<FilePath>,
    docs: BTreeMap<FilePath, Doc>,
    changes: Vec<ChangeDto>,
}

#[derive(Deserialize)]
struct PathArgs {
    path: String,
}

#[derive(Deserialize)]
struct ApplyArgs {
    path: String,
    mutations: Vec<MutationDto>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct HistoryArgs {
    path: String,
    #[serde(default)]
    task_id: Option<String>,
    #[serde(default)]
    limit: Option<usize>,
}

fn args<T: DeserializeOwned>(cmd: &str, value: &Value) -> Result<T, WebError> {
    serde_json::from_value(value.clone()).map_err(|e| WebError::BadArgs(format!("{cmd}: {e}")))
}

fn json<T: serde::Serialize>(value: &T) -> Result<Value, WebError> {
    serde_json::to_value(value).map_err(|e| WebError::BadArgs(format!("result: {e}")))
}

fn file_path(text: &str) -> Result<FilePath, WebError> {
    FilePath::new(text).map_err(|e| WebError::BadArgs(format!("path {text:?}: {e}")))
}

impl Host {
    /// Opens the host over `store`: this device's id (minted the first time) and the known lists
    /// (the root list the first time).
    pub fn open(mut store: Store) -> Result<Host, WebError> {
        let device = match store.meta_get(DEVICE_KEY)? {
            Some(bytes) if bytes.len() == 16 => {
                let mut b = [0u8; 16];
                b.copy_from_slice(&bytes);
                DeviceId::new(Ulid::from_u128(u128::from_be_bytes(b)))
            }
            _ => {
                let id = new_ulid(now_ms());
                store.meta_set(DEVICE_KEY, &id.to_u128().to_be_bytes())?;
                DeviceId::new(id)
            }
        };
        let listed = store.meta_get(FILES_KEY)?.unwrap_or_default();
        let mut files: Vec<FilePath> = String::from_utf8_lossy(&listed)
            .lines()
            .filter_map(|l| FilePath::new(l).ok())
            .take(MAX_LISTS)
            .collect();
        if files.is_empty() {
            files.push(file_path(ROOT_LIST)?);
        }
        Ok(Host {
            store,
            device,
            files,
            docs: BTreeMap::new(),
            changes: Vec::new(),
        })
    }

    /// This device.
    pub fn device(&self) -> DeviceId {
        self.device
    }

    /// Answers one command; its result as JSON.
    pub fn call(&mut self, cmd: &str, a: &Value) -> Result<Value, WebError> {
        match cmd {
            "list_files" => self.list_files(),
            "get_file" => self.get_file(&args::<PathArgs>(cmd, a)?.path),
            "apply" => self.apply(args(cmd, a)?),
            "history" => self.history(args(cmd, a)?),
            "list_duplicates" => self.list_duplicates(&args::<PathArgs>(cmd, a)?.path),
            // Changes reach the page as events (`take_changes`); there is no stream to open.
            "watch" => Ok(Value::Null),
            "undo_change" => Err(WebError::Unsupported("undo")),
            other => Err(WebError::UnknownCommand(other.to_owned())),
        }
    }

    /// The change events since the last call, oldest first.
    pub fn take_changes(&mut self) -> Vec<ChangeDto> {
        std::mem::take(&mut self.changes)
    }

    /// The open list at `path`, opened (and from then on listed) on first use.
    fn doc(&mut self, path: &str) -> Result<&mut Doc, WebError> {
        let path = file_path(path)?;
        if !path.as_str().ends_with(".txt") {
            return Err(WebError::Unsupported(
                "a document that is not a todo.txt list",
            ));
        }
        if !self.docs.contains_key(&path) {
            let doc = Doc::open(&self.store, path.clone(), self.device)?;
            self.docs.insert(path.clone(), doc);
        }
        if !self.files.contains(&path) {
            if self.files.len() >= MAX_LISTS {
                return Err(WebError::Unsupported("more lists than MAX_LISTS"));
            }
            self.files.push(path.clone());
            let text: String = self.files.iter().map(|f| format!("{f}\n")).collect();
            self.store.meta_set(FILES_KEY, text.as_bytes())?;
        }
        self.docs
            .get_mut(&path)
            .ok_or(WebError::Unsupported("a list that would not open"))
    }

    fn list_files(&mut self) -> Result<Value, WebError> {
        let paths: Vec<String> = self.files.iter().map(|f| f.to_string()).collect();
        let mut out = Vec::with_capacity(paths.len());
        for path in paths {
            let doc = self.doc(&path)?;
            let counts = doc.state().task_counts();
            out.push(FileInfoDto {
                hash: hex(&doc.hash()),
                kind: "FILE_KIND_TODO".to_owned(),
                done: u32::try_from(counts.completed).unwrap_or(u32::MAX),
                total: u32::try_from(counts.total).unwrap_or(u32::MAX),
                path,
            });
        }
        json(&out)
    }

    fn get_file(&mut self, path: &str) -> Result<Value, WebError> {
        let c = self.doc(path)?.contents();
        json(&FileContentsDto {
            path: path.to_owned(),
            text: String::from_utf8_lossy(&c.bytes).into_owned(),
            hash: hex(&c.hash),
            task_ids: c.task_id_texts(),
        })
    }

    fn apply(&mut self, a: ApplyArgs) -> Result<Value, WebError> {
        let today = today_local();
        let mutations = (a.mutations.into_iter())
            .map(|m| m.into_mutation(today))
            .collect::<Result<Vec<_>, _>>()?;
        let by = By {
            principal: Principal::User {
                device: self.device,
            },
            now: now_ms(),
        };
        self.doc(&a.path)?;
        let Host {
            store,
            docs,
            changes,
            ..
        } = self;
        let doc = docs
            .get_mut(&file_path(&a.path)?)
            .ok_or(WebError::Unsupported("a list that would not open"))?;
        let applied = doc.apply(store, &mutations, &by)?;
        if !applied.ops.is_empty() {
            if changes.len() >= MAX_PENDING_CHANGES {
                changes.remove(0);
            }
            changes.push(ChangeDto {
                path: a.path,
                hash: hex(&applied.hash),
                // The page reads the ops' kinds and ids; their seqs are the store's business.
                ops: (applied.ops.iter())
                    .map(|op| OpSummaryDto::of(0, op))
                    .collect(),
                duplicate_groups: u32::try_from(duplicate_groups(doc.state()).len()).unwrap_or(0),
            });
        }
        json(&ApplyResultDto {
            applied: u32::try_from(applied.ops.len()).unwrap_or(u32::MAX),
            hash: hex(&applied.hash),
            hlc_wall_ms: applied.hlc.wall_ms,
            hlc_counter: u32::from(applied.hlc.counter),
        })
    }

    fn history(&mut self, a: HistoryArgs) -> Result<Value, WebError> {
        let path = file_path(&a.path)?;
        let task = a.task_id.filter(|t| !t.is_empty());
        let ops = all_ops(&self.store, &path)?;
        let picked = ops.iter().rev().filter(|s| match &task {
            Some(t) => task_of(&s.op.kind).is_some_and(|id| id.to_string() == *t),
            None => true,
        });
        let ops: Vec<OpSummaryDto> = picked
            .take(a.limit.unwrap_or(usize::MAX))
            .map(OpSummaryDto::stored)
            .collect();
        json(&HistoryDto { ops })
    }

    fn list_duplicates(&mut self, path: &str) -> Result<Value, WebError> {
        let groups: Vec<DuplicateGroupDto> = duplicate_groups(self.doc(path)?.state())
            .iter()
            .map(DuplicateGroupDto::from)
            .collect();
        json(&groups)
    }
}
