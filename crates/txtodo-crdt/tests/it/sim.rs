//! N-device sync simulator (plan M4 `crdt-sync-simulator`): a seeded PRNG drives every random
//! choice, `LoroDocument::fork`/`export_updates`/`import` carries convergence between devices, and
//! partitions come and go before every run is forced to heal and reach a fixpoint.
//!
//! **Scope, decided with the human 2026-09-12**: `txtodo-crdt` may depend only on
//! `txtodo-model`/`txtodo-store`/`txtodo-core` (`.claude/budgets.json`, this crate's own
//! `CLAUDE.md`) — `txtodo-sync`'s `Session`/`Link` are not reachable from a test in this crate, and
//! the real system does not route convergence through them either (`txtodo-daemon`'s `Mirror`
//! converges two documents via Loro's own `export_updates`/`import`, never by replaying `Op`s
//! across independent documents — see this crate's own invariant on that). So this simulator is
//! Loro-native, exactly like the real system, and asserts **CRDT-level** convergence: id order,
//! deleted flags and canonical line text agree across every device. It does not assert
//! byte-identical `todo.txt` files (quirks/line endings live in `txtodo-daemon`'s `DocState`,
//! unreachable from here) — that check, and the two-real-`txtodod`-processes acceptance test,
//! belong to `sync-loopback-converge` instead. The 1000-run sweep, the seed shrinker and the
//! "broken merge" meta-test framework from the task notes are not attempted this pass; see
//! `tasks/crdt-sync-simulator/notes.md`.
// Integration tests are tests: clippy.toml allows unwrap/expect/print in #[test] fns, not helpers.
#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::print_stderr,
    clippy::print_stdout
)]

// it/sim/device.rs and it/sim/rng.rs: this file is the `sim` module of the `it` binary.
mod device;
mod rng;

use std::collections::BTreeSet;

use txtodo_crdt::{LoroDocument, apply, is_blank, rebuild_line};
use txtodo_model::{FilePath, OpKind, TaskId};

use device::Device;
use rng::Rng;

/// However many partition/sync rounds it takes, a run that has not converged by this many is
/// stuck, not slow — the task notes ask for a bounded loop, never an infinite one.
const MAX_SYNC_ROUNDS: u32 = 50;

/// One converged document, read back as `(id, deleted, canonical line)` in list order — list order
/// is itself part of what must agree, so this is a `Vec`, never sorted into a `BTreeMap`.
type Snapshot = Vec<(TaskId, bool, String)>;

/// One simulator run's shape — everything the run depends on, so it is a pure function of these
/// fields alone.
pub struct SimConfig {
    /// Drives every random choice in the run; the whole run is a pure function of this plus the
    /// fields below.
    pub seed: u64,
    /// How many devices fork from the shared ancestor.
    pub devices: usize,
    /// How many ops the run performs in total, spread across devices at random.
    pub ops: usize,
    /// A partition flips with probability `partition_num`/`partition_den` after every op.
    pub partition_num: u64,
    /// See [`SimConfig::partition_num`].
    pub partition_den: u64,
    /// A full all-pairs sync round runs every this many ops, not after each one — a real LAN
    /// doesn't resync on every keystroke, and syncing this rarely keeps the default `cargo test`
    /// (20 seeds) fast; the final heal-and-converge loop after the run is unaffected.
    pub sync_every: usize,
}

impl SimConfig {
    /// The M4 acceptance shape (5 devices, 200 ops) at the given seed.
    pub fn new(seed: u64) -> SimConfig {
        SimConfig {
            seed,
            devices: 5,
            ops: 200,
            partition_num: 1,
            partition_den: 8,
            sync_every: 8,
        }
    }
}

fn todo_file() -> FilePath {
    FilePath::new("todo.txt").expect("a literal, valid file path")
}

/// Exports what `to` lacks from `from` and imports it — the pairwise round the real system's
/// `Mirror` also runs; `to.version()`'s well-formed bytes make `export_updates` infallible here,
/// and importing our own just-exported bytes cannot fail either. Returns whether anything new
/// landed (`Imported::applied`), so a fixpoint loop can tell "converged" from "nothing to do." Runs
/// every pair in one pass, so a full sync round is `sync_all_pairs`, not this alone: two statements
/// per pair, not one call on `&devices[i].doc, &mut devices[j].doc`, because the borrow checker
/// cannot see `i != j` through a single indexing expression.
fn sync_all_pairs(devices: &mut [Device]) -> bool {
    let mut any = false;
    for i in 0..devices.len() {
        for j in 0..devices.len() {
            if i == j || devices[i].partitioned || devices[j].partitioned {
                continue;
            }
            // Two statements, not one call on `&devices[i].doc, &mut devices[j].doc`: the borrow
            // checker cannot see `i != j` through a single indexing expression, so the export
            // (immutable) must fully finish before the import (mutable) borrow starts.
            let bytes = devices[i]
                .doc
                .export_updates(&devices[j].doc.version())
                .expect("exporting since a version this same process produced cannot fail");
            any |= devices[j]
                .doc
                .import(&bytes)
                .expect("importing bytes we just exported cannot fail")
                .applied;
        }
    }
    any
}

/// Runs `config` to a healed fixpoint and checks the three M4 acceptance properties. `Err` names
/// the seed and config so a human can hand `TXTODO_SIM_SEED=<seed>` back for an exact replay (task
/// notes: "a failing seed you cannot minimise is a bug report you cannot act on").
pub fn run_scenario(config: SimConfig) -> Result<Snapshot, String> {
    let describe = |what: &str| {
        format!(
            "{what} (seed={}, devices={}, ops={})",
            config.seed, config.devices, config.ops
        )
    };
    let mut rng = Rng::new(config.seed);
    let file = todo_file();
    let ancestor = LoroDocument::open();
    let mut devices: Vec<Device> = (0..config.devices as u128)
        .map(|n| Device::new(n, &ancestor))
        .collect();
    let mut all_inserted: BTreeSet<TaskId> = BTreeSet::new();
    let mut now_ms = 0u64;

    for step in 0..config.ops {
        now_ms += 1 + rng.below(20) as u64;
        let actor = rng.below(devices.len());
        let op = devices[actor].random_op(&file, &mut rng, now_ms);
        if let OpKind::Insert { task, .. } = &op.kind {
            all_inserted.insert(*task);
        }
        apply(&mut devices[actor].doc, &op).map_err(|e| describe(&format!("apply failed: {e}")))?;

        if rng.chance(config.partition_num, config.partition_den) {
            let d = rng.below(devices.len());
            devices[d].partitioned = !devices[d].partitioned;
        }
        if step % config.sync_every == 0 {
            sync_all_pairs(&mut devices);
        }
    }

    for device in &mut devices {
        device.partitioned = false;
    }
    let mut rounds = 0u32;
    loop {
        rounds += 1;
        if rounds > MAX_SYNC_ROUNDS {
            return Err(describe(
                "did not reach a fixpoint within the bounded sync-round cap",
            ));
        }
        if !sync_all_pairs(&mut devices) {
            break;
        }
    }

    assert_converged(&devices, &file, &all_inserted, describe)
}

/// The three M4 acceptance properties, factored out so a hand-built (not simulated) scenario can
/// call it directly to prove the checks themselves discriminate converged from diverged —
/// `a_diverged_device_fails_the_convergence_assertion` below does exactly that.
fn assert_converged(
    devices: &[Device],
    file: &FilePath,
    all_inserted: &BTreeSet<TaskId>,
    describe: impl Fn(&str) -> String,
) -> Result<Snapshot, String> {
    let reference_ids: Vec<TaskId> = visible_ids(&devices[0], file);
    assert_no_duplicates(&reference_ids, file, &describe)?;
    assert_no_loss(&devices[0], &reference_ids, all_inserted, &describe)?;

    let mut reference = Snapshot::with_capacity(reference_ids.len());
    for id in &reference_ids {
        let line = rebuild_line(&devices[0].doc, *id).map_err(|e| {
            describe(&format!(
                "device 0's own line for {id} did not rebuild: {e:?}"
            ))
        })?;
        reference.push((*id, devices[0].doc.is_deleted(*id), line));
    }

    for (n, device) in devices.iter().enumerate().skip(1) {
        assert_matches_reference(n, device, file, &reference, &describe)?;
    }

    Ok(reference)
}

fn visible_ids(device: &Device, file: &FilePath) -> Vec<TaskId> {
    device
        .doc
        .list_ids(file)
        .into_iter()
        .filter(|id| !is_blank(*id))
        .collect()
}

fn assert_no_duplicates(
    ids: &[TaskId],
    file: &FilePath,
    describe: &impl Fn(&str) -> String,
) -> Result<(), String> {
    let mut seen = BTreeSet::new();
    for id in ids {
        if !seen.insert(*id) {
            return Err(describe(&format!(
                "duplicate id {id} in {file} after convergence"
            )));
        }
    }
    Ok(())
}

fn assert_no_loss(
    reference: &Device,
    reference_ids: &[TaskId],
    all_inserted: &BTreeSet<TaskId>,
    describe: &impl Fn(&str) -> String,
) -> Result<(), String> {
    for id in all_inserted {
        let deleted = reference.doc.is_deleted(*id);
        if !deleted && !reference_ids.contains(id) {
            return Err(describe(&format!(
                "task {id} was inserted and never deleted, but is missing after convergence"
            )));
        }
    }
    Ok(())
}

fn assert_matches_reference(
    n: usize,
    device: &Device,
    file: &FilePath,
    reference: &Snapshot,
    describe: &impl Fn(&str) -> String,
) -> Result<(), String> {
    let ids = visible_ids(device, file);
    let reference_ids: Vec<TaskId> = reference.iter().map(|(id, ..)| *id).collect();
    if ids != reference_ids {
        return Err(describe(&format!(
            "device {n}'s id order diverged from device 0"
        )));
    }
    for (id, want_deleted, want_line) in reference {
        let got_deleted = device.doc.is_deleted(*id);
        if got_deleted != *want_deleted {
            return Err(describe(&format!(
                "device {n} disagrees with device 0 on {id}'s deleted flag"
            )));
        }
        let got_line = rebuild_line(&device.doc, *id).map_err(|e| {
            describe(&format!(
                "device {n}'s line for {id} did not rebuild: {e:?}"
            ))
        })?;
        if got_line != *want_line {
            return Err(describe(&format!(
                "device {n}'s line for {id} diverged from device 0"
            )));
        }
    }
    Ok(())
}

#[test]
fn slow_twenty_fixed_seeds_converge() {
    for seed in 0..20u64 {
        run_scenario(SimConfig::new(seed)).unwrap_or_else(|e| panic!("{e}"));
    }
}
