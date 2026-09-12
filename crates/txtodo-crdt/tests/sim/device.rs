//! One simulated device: a `LoroDocument` sharing lineage with every other device (all forked from
//! one common ancestor, per `txtodo-crdt`'s own invariant that replaying `Op`s into independent
//! documents does not converge — only Loro's own `export_updates`/`import` does), plus the local
//! state a real device would have: its own HLC and whether it is currently partitioned from the
//! others.

use txtodo_crdt::{LoroDocument, is_blank};
use txtodo_model::{
    DeviceId, Field, FieldValue, FilePath, Hlc, Op, OpId, OpKind, Principal, TaskId, TextEdit,
    Ulid, set_field,
};

use super::rng::Rng;

/// A handful of plain descriptions — no leading `x `/`(A)`/tag-looking tokens, so the generated
/// line always parses as a bare task regardless of where the PRNG splices words in or out.
const WORDS: &[&str] = &[
    "milk", "bread", "eggs", "report", "invoice", "backup", "ship", "review", "call", "walk",
];

pub struct Device {
    pub id: DeviceId,
    pub doc: LoroDocument,
    pub hlc: Hlc,
    pub partitioned: bool,
}

impl Device {
    /// Forks `ancestor` and pins a peer id derived from `n` — distinct per device, as Loro
    /// requires for lineage-sharing documents (`crdt-sync-simulator` notes; `LoroDocument::CLAUDE.md`
    /// invariant).
    pub fn new(n: u128, ancestor: &LoroDocument) -> Device {
        let id = DeviceId::new(Ulid::from_u128(n));
        let doc = ancestor.fork();
        doc.set_peer(n as u64).expect("peer id set once, up front");
        Device {
            id,
            doc,
            hlc: Hlc::zero(id),
            partitioned: false,
        }
    }

    /// This device's own view of what it can still edit: not deleted, not a blank sentinel.
    pub fn live_tasks(&self, file: &FilePath) -> Vec<TaskId> {
        self.doc
            .list_ids(file)
            .into_iter()
            .filter(|id| !is_blank(*id) && !self.doc.is_deleted(*id))
            .collect()
    }

    /// One op this device originates, stamped with its own ticking HLC. Never references a task
    /// this device cannot itself see — a partitioned device only knows what it knew before the
    /// partition (or has generated itself since), matching what a real offline device could do.
    pub fn random_op(&mut self, file: &FilePath, rng: &mut Rng, now_ms: u64) -> Op {
        let live = self.live_tasks(file);
        let kind = choose_kind(&live, rng, now_ms);
        let hlc = self
            .hlc
            .tick(now_ms)
            .expect("simulated clock only advances");
        Op {
            id: OpId::new(mint_ulid(now_ms, rng)),
            hlc,
            principal: Principal::External { device: self.id },
            file: file.clone(),
            kind,
        }
    }
}

/// A ULID from the simulated clock plus PRNG entropy, mirroring `txtodo-daemon`'s
/// `clock::ulid_from` pattern (that module isn't reachable from `txtodo-crdt`, so this is a
/// from-scratch, test-only reimplementation, not a copy). `now_ms` stays far below 2^40 for any
/// run this simulator drives, so the top byte of the 48-bit timestamp field is always `0x00` —
/// never `0xFF`, the blank-sentinel prefix (`doc.rs::BLANK_PREFIX_MASK`) — without needing to mask
/// it explicitly.
fn mint_ulid(now_ms: u64, rng: &mut Rng) -> Ulid {
    let random: [u8; 10] = rng.bytes();
    let entropy = random
        .iter()
        .fold(0u128, |acc, b| (acc << 8) | u128::from(*b));
    let ms48 = u128::from(now_ms) & ((1u128 << 48) - 1);
    debug_assert!(
        now_ms < (1u64 << 40),
        "a simulated run must never reach the blank-sentinel top byte"
    );
    Ulid::from_u128((ms48 << 80) | entropy)
}

fn word(rng: &mut Rng) -> &'static str {
    WORDS[rng.below(WORDS.len())]
}

/// Insert with no live tasks yet; otherwise a weighted pick among insert/complete/delete/edit.
fn choose_kind(live: &[TaskId], rng: &mut Rng, now_ms: u64) -> OpKind {
    if live.is_empty() || rng.chance(1, 3) {
        let after = if live.is_empty() {
            None
        } else {
            Some(live[rng.below(live.len())])
        };
        let task = TaskId::new(mint_ulid(now_ms, rng));
        return OpKind::Insert {
            task,
            after,
            line: format!("{} {} id:{}", word(rng), word(rng), task.ulid()),
        };
    }
    let task = live[rng.below(live.len())];
    match rng.below(3) {
        0 => set_field(task, Field::Completed, FieldValue::Bool(true))
            .expect("Completed/Bool is a valid pairing"),
        1 => set_field(task, Field::Deleted, FieldValue::Bool(true))
            .expect("Deleted/Bool is a valid pairing"),
        _ => OpKind::EditText {
            task,
            edits: vec![TextEdit::Insert {
                at: 0,
                text: format!("{} ", word(rng)),
            }],
        },
    }
}
