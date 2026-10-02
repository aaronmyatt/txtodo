//! An order-free hash of one document's op set (task sync-divergence-check). Two devices that
//! hold the same ops for a file have the same `OpSetHash`, whatever order the ops arrived in, so
//! "op sets equal but bytes differ" can be told apart from "a peer is still behind".
//!
//! It is the XOR of blake3 over each op id's 16 raw bytes (the big-endian ULID, the same form the
//! store keeps in `ops.op_id`). XOR is commutative and associative, so arrival order cannot change
//! it; an op id is `UNIQUE` in the log, so no op is ever folded in twice. Workspace `Heads` are not
//! a substitute: they are per workspace, and an op's origin rank can shift.
//!
//! Kept by `FileActor` and `NotesActor`: rebuilt from the store when the actor opens, then folded
//! forward with every op their single commit point lands. The op log is append-only, so nothing
//! ever has to be taken out.
//! blake3: https://docs.rs/blake3/latest/blake3/fn.hash.html

use std::fmt;

use txtodo_model::{FilePath, Op, OpId};
use txtodo_store::{Store, StoreError};

/// The XOR of `blake3(op id)` over every op in one document. `default()` is the empty set.
#[derive(Clone, Copy, Default, PartialEq, Eq, Hash)]
pub struct OpSetHash([u8; 32]);

impl OpSetHash {
    /// Every op of `file` in `store`: what an actor starts from when it opens.
    pub fn of_file(store: &Store, file: &FilePath) -> Result<OpSetHash, StoreError> {
        let mut set = OpSetHash::default();
        store.for_each_op_id_of_file(file, |raw| set.add_raw(raw))?;
        Ok(set)
    }

    /// Folds in ops that just became durable. Each must be new to this document.
    pub fn add_ops(&mut self, ops: &[Op]) {
        for op in ops {
            self.add(op.id);
        }
    }

    /// Folds in one op.
    pub fn add(&mut self, id: OpId) {
        self.add_raw(id.ulid().to_u128().to_be_bytes());
    }

    fn add_raw(&mut self, raw: [u8; 16]) {
        let h = blake3::hash(&raw);
        for (mine, theirs) in self.0.iter_mut().zip(h.as_bytes()) {
            *mine ^= theirs;
        }
    }

    /// The 32 bytes, for the wire or a log line.
    pub fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }
}

/// The first 8 hex digits, like the projection hashes in this crate's logs.
impl fmt::Debug for OpSetHash {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for b in &self.0[..4] {
            write!(f, "{b:02x}")?;
        }
        Ok(())
    }
}
