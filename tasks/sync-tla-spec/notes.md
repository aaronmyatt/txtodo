# TLA+ spec specs/sync.tla with TLC run in CI (plan M10)

Goal: TLA+ model of devices, op log, and reconciler; TLC checks convergence and no-loss in CI.

Design: txtodo-design.md §11 — `specs/sync.tla`; TLC checks convergence (all online devices reach the same file) and the no-loss invariant.

Plan: txtodo-implementation-plan.md M10 — mini-plan deferred until started.
