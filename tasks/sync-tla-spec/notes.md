# TLA+ sync spec (specs/sync.tla) + TLC in CI (plan M10, design §11)

## Goal

A TLA+ model of devices, the op log, and the reconciler at `specs/sync.tla`, with TLC run in CI
checking convergence and no-loss. Design §11: "models devices, the op log, and the reconciler; TLC
checks convergence (all online devices reach the same file) and the no-loss invariant." This is a
*specification of the sync protocol* (M4's Hello/Want/Ops/Ack + the reconciler), not of Loro's
internals — the CRDT is abstracted as "apply op → deterministic state".

## Design

Module `Sync`, config `Sync.cfg`, run via `tlc Sync -config specs/sync.cfg`. Constants: `Device`
(a finite set of device ids), `MaxOps` (bounded model). State = per-device op log and the
materialised file (an ordered sequence of task lines); ops carry a unique id so the model can speak
about "the same op".

```tla
---- MODULE Sync ----
EXTENDS Integers, Sequences, FiniteSets, TLC
CONSTANTS Device, MaxOps
VARIABLES oplog,     \* oplog[d] : sequence of ops this device originated
          applied,   \* applied[d] : set of op ids this device has integrated
          file       \* file[d] : the materialised line list

Init == /\ oplog  = [d \in Device |-> <<>>]
        /\ applied = [d \in Device |-> {}]
        /\ file   = [d \in Device |-> <<>>]

AddOp(d)      == \E op \in NewOps: /\ oplog'  = [oplog  EXCEPT ![d] = Append(@, op)]
                                   /\ applied' = [applied EXCEPT ![d] = @ \cup {op.id}]
                                   /\ file'    = [file    EXCEPT ![d] = Apply(file[d], op)]
SyncOps(a, b) == \* b sends a the ops it lacks; a integrates them (the Want/Ack round)
                 /\ \E missing \in SUBSET (oplog[b] \ applied[a]):
                      /\ applied' = [applied EXCEPT ![a] = @ \cup {op.id : op \in missing}]
                      /\ file'    = [file    EXCEPT ![a] = FoldApply(file[a], missing)]
                 /\ UNCHANGED oplog

Next == \E d \in Device: AddOp(d) \/ \E a,b \in Device: SyncOps(a, b)

TypeOK       == /\ \A d \in Device: Seq(file[d])
                /\ \A d \in Device: applied[d] \subseteq {op.id : op \in oplog[d]} ...
Convergence  == \A a,b \in Device: file[a] = file[b]        \* all devices reach the same file
NoLoss       == \A d \in Device, op \in oplog[d]: op.id \in applied[d]   \* every op lands (no drop)
```

(`Apply`/`FoldApply` are ordinary operators over a line sequence; `NewOps` is a finite set of
distinct ops sized by `MaxOps`. TLC model-checks `Init /\ [][Next]_vars` against `TypeOK`,
`Convergence` (checked as a liveness/stability property under a weak-fairness condition that every
pair eventually syncs), and `NoLoss`.)

- `specs/sync.cfg`: `CONSTANT Device = {d1,d2,d3}`, `MaxOps = 4`, `CHECK_DEADLOCK = FALSE`,
  `SPECIFICATION Spec`, and the invariants/properties above. Bounded, so TLC terminates.
- The convergence property mirrors the M4 simulator's guarantee ("all online devices reach the same
  file"); the spec is the *design-time* check, the simulator the *runtime* check — they must agree.
- Negative control: a mutant module `SyncDrop` (a device that never applies a `Want` it received) is
  checked in CI too and must FAIL `NoLoss` with a counterexample — proving the invariant is not
  vacuous (TLC prints the trace).

## Placement/dependencies

- `specs/sync.tla` + `specs/sync.cfg` — `specs/**` is a FROZEN path (budgets.json frozenPaths): ask
  before writing, every time (mirrors how `todotxt.abnf` was fenced in M0).
- CI: a `tlc` job in `.github/workflows/ci.yml` (also frozen) downloads the pinned `tla2tools.jar`
  (tlaplus) and runs `tlc Sync` plus the `SyncDrop` negative check. No new Rust dep; TLC is a JVM
  tool, pinned by checksum.

## Edge cases & invariants

- TLC is a bounded checker: `Device = {d1,d2,d3}` and `MaxOps = 4` keep the state space small; a
  larger model is a separate, slower job (`MaxOps = 8`) gated on the small one.
- Fairness: `Convergence` must be checked under weak fairness (`WF_vars(SyncOps(a,b))`) or it is
  trivially false when one device simply never syncs — the spec must state the liveness assumption,
  not bury it.
- The op log and the file must stay consistent in the model (`TypeOK` asserts every `file[d]` is
  derivable from `applied[d]`), mirroring "the file is the truth" (design §0) at the model level.

## Acceptance

- `tlc Sync -config specs/sync.cfg` passes: `TypeOK`, `Convergence`, and `NoLoss` all hold on the
  3-device / 4-op model, with no deadlock.
- The `SyncDrop` mutant FAILS `NoLoss` and TLC prints a counterexample trace (checked in CI as a
  "must-fail" step, like the fuzz canary).
- The CI `tlc` job runs on every push; a spec change that breaks `Convergence` or `NoLoss` fails the
  build.
- The spec's `Convergence` statement matches the M4 simulator's assertion (all online devices reach
  the same file) — documented by a comment in the .tla referencing `txtodo-crdt/tests/sim.rs`.

## Frozen paths touched

- `specs/**` (specs/sync.tla, specs/sync.cfg) — frozen, ask before writing.
- `.github/**` (ci.yml `tlc` job) — frozen, ask before writing.

## References

- plan M10 (txtodo-implementation-plan.md), design §11 (txtodo-design.md)
- TLA+ / TLC: https://lamport.azurewebsites.net/tla/tla.html · https://github.com/tlaplus/tlaplus
- TLC CLI + .cfg: https://tla.msr-inria.fr/tlatoolbox/documentation/
