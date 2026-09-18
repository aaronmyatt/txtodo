# daemon-paths-shared-crate

## Reported

Auditing daemon-startup flakiness across clients (`txtodo`, `txtodo-tui`, `txtodo-mcp`,
`apps/desktop`) found the socket/pid/registry path resolution logic implemented three separate
times instead of once, which is a drift risk even though no divergence has been observed yet.

## Current state

- Canonical implementation: `crates/txtodo-daemon/src/workspace_registry_paths.rs:59-108`.
- Reimplemented independently in `crates/txtodo-cli/src/config.rs:58+` and
  `crates/txtodo-mcp/src/global_socket.rs:16-59`.
- `global_socket.rs`'s own doc comment explains why: `budgets.json`'s `allowedDeps` forbids
  `txtodo-mcp` (and `txtodo-cli`) from depending on `txtodo-daemon`, so each reimplements the same
  fallback chain (state dir → registry file → default) by hand.
- This is documented as deliberate, not accidental — but "deliberate" only means the risk was
  known, not that it's safe. If the fallback chain in `workspace_registry_paths.rs` ever changes
  (new env var, new default location, new precedence order), the other two copies silently keep
  the old behavior until someone remembers to update them by hand.
- `crates/txtodo-daemon-launch` already exists as a dependency-light leaf crate (built for
  `ref:daemon-always-available`, `#[cfg(unix)]` + stub, zero `txtodo-daemon` dependency) and is
  already consumed by `txtodo-cli`, `txtodo-tui`, `txtodo-mcp`, and `apps/desktop` — proof this
  dependency shape is already accepted by `allowedDeps`.

## Design

- Extract the path-resolution logic out of `workspace_registry_paths.rs` into
  `txtodo-daemon-launch` (or a new leaf crate next to it, if `daemon-launch` itself needs to stay
  narrowly scoped to spawn/service concerns) — same shape as the spawn-helper extraction already
  did for `apps/desktop`'s `ensure_daemon`.
- Point `crates/txtodo-daemon/src/workspace_registry_paths.rs`, `crates/txtodo-cli/src/config.rs`,
  and `crates/txtodo-mcp/src/global_socket.rs` all at the shared implementation, deleting the two
  duplicate fallback chains.
- Keep the public function signatures stable enough that this is a mechanical swap, not a
  behavior change — no path should resolve differently after this lands.

## Out of scope

- Changing the actual fallback chain / precedence order — this task only removes duplication, it
  doesn't redesign path resolution.

## Acceptance

- Only one implementation of the socket/pid/registry fallback chain exists in the workspace.
- `cli`, `mcp`, and `daemon` all resolve identical paths for identical inputs (covered by a
  cross-crate test comparing resolved paths under the same env/config).

## As built (2026-09-18)

- Found a 4th copy while auditing: `apps/desktop/src-tauri/src/config.rs` reimplemented the same
  chain too, not just cli/mcp — its own doc comment said as much ("mirrors txtodo-daemon's
  workspace_registry_paths... and txtodo-cli's own copy").
- New crate `crates/txtodo-workspace-paths` (leaf, zero deps): `workspace_registry_paths.rs`'s
  content moved verbatim (`f7ea5d6`), plus `global_state_dir` made `pub` (`0c0472b`) so
  `apps/desktop` could reuse it too, not just the pid/log paths already built from it.
- Required a human-authorized `.claude/UNFROZEN` touch: root `Cargo.toml` and `.claude/budgets.json`
  (`slices.allowedDeps`) are frozen paths, and adding a new crate + its permitted dependents
  touches both. Per-crate `Cargo.toml` files are *not* frozen, so the four migration commits
  themselves needed no further unlock.
- All four call sites migrated, one commit each (the repo's slice-fence allows one leased crate
  at a time per session):
  - `txtodo-daemon`: `workspace_registry_paths` is now `pub use txtodo_workspace_paths as
    workspace_registry_paths;` — every existing `txtodo_daemon::workspace_registry_paths::*` call
    site kept working unchanged (`b4aa410`).
  - `txtodo-cli`: `config.rs::global_socket_path` delegates, wrapping its own `Env` into a
    `RegistryEnv` at the call site (`00aa5a8`).
  - `txtodo-mcp`: `global_socket.rs` rewritten to two thin wrappers; dropped its own now-redundant
    unit tests since the shared crate's 8 cover the same fallback chain (`4ce7e8a`).
  - `apps/desktop`: `config.rs`'s `global_socket_path`/`global_state_dir` both delegate (`1e46120`).
- Acceptance's "cross-crate test comparing resolved paths" wasn't built as a separate test: since
  all four now call the literal same shared function, identical resolution is structural (one
  code path), not something that needs a runtime test to prove. Noted as a deliberate
  interpretation change, not a skipped item.
- `cargo build/clippy/test` and `check-boundaries.sh` all green for every crate touched, after
  each individual commit.
