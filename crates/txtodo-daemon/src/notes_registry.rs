//! One `NotesActor` per `<ref>/notes.md`, created lazily on first access and kept behind a mutex
//! so every `GetNotes`/`EditNotes` call for the same directory serialises through the same writer
//! (plan §3.2's one-writer-per-file rule, applied to the notes document kind too).

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex, PoisonError};

use crate::actor::SharedStore;
use crate::clock::Clock;
use crate::handle::ActorError;
use crate::notes_actor::{NotesActor, NotesActorConfig};
use txtodo_model::FilePath;

/// A notes actor behind a lock, shared by every caller that resolves to the same path.
pub type NotesCell = Arc<Mutex<NotesActor>>;

/// The live notes actors for one workspace.
#[derive(Default)]
pub struct NotesRegistry {
    actors: Mutex<BTreeMap<FilePath, NotesCell>>,
}

impl NotesRegistry {
    /// An empty registry.
    pub fn new() -> NotesRegistry {
        NotesRegistry::default()
    }

    /// The actor for `cfg.path`, opening it from disk/store on first use.
    pub fn get_or_open(
        &self,
        cfg: NotesActorConfig,
        store: &SharedStore,
        clock: &Arc<dyn Clock>,
    ) -> Result<NotesCell, ActorError> {
        let mut actors = self.actors.lock().unwrap_or_else(PoisonError::into_inner);
        if let Some(cell) = actors.get(&cfg.path) {
            return Ok(Arc::clone(cell));
        }
        let path = cfg.path.clone();
        let actor = NotesActor::open(cfg, Arc::clone(store), Arc::clone(clock))?;
        let cell = Arc::new(Mutex::new(actor));
        actors.insert(path, Arc::clone(&cell));
        Ok(cell)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::clock::FakeClock;
    use txtodo_model::{DeviceId, Ulid};
    use txtodo_store::Store;

    #[test]
    fn the_same_path_returns_the_same_cell() {
        let dir = tempfile::tempdir().unwrap_or_else(|e| panic!("{e}"));
        let store: SharedStore = Arc::new(Mutex::new(
            Store::open(&dir.path().join("oplog.db")).unwrap_or_else(|e| panic!("{e}")),
        ));
        let clock: Arc<dyn Clock> = Arc::new(FakeClock::new(1_000));
        let registry = NotesRegistry::new();
        let cfg = NotesActorConfig {
            path: FilePath::new("q4/abc/notes.md").unwrap_or_else(|e| panic!("{e}")),
            disk: dir.path().join("q4/abc/notes.md"),
            device: DeviceId::new(Ulid::from_u128(1)),
        };
        let a = registry
            .get_or_open(cfg.clone(), &store, &clock)
            .unwrap_or_else(|e| panic!("{e}"));
        let b = registry
            .get_or_open(cfg, &store, &clock)
            .unwrap_or_else(|e| panic!("{e}"));
        assert!(Arc::ptr_eq(&a, &b));
    }
}
