//! Shared "make sure `txtodod` exists" logic (task `daemon-always-available`): every client
//! (`txtodo-cli`, `txtodo-tui`, `txtodo-mcp`, `apps/desktop`) optimistically ensures a daemon is
//! reachable before it needs one, instead of erroring the moment a socket is missing. This crate
//! covers both halves of "always available":
//!
//! - **Ad-hoc spawn** ([`ensure_daemon`]): ported out of `apps/desktop/src-tauri/src/daemon/
//!   spawn.rs`, generalized over the target socket, the daemon binary and its argv (a legacy
//!   per-workspace bridge daemon takes `--dir <workspace>`; the ADR 0025 global daemon takes
//!   none) so every client can use the same probe/lock/spawn/wait sequence.
//! - **Persistent boot service** ([`service`]): the launchd/systemd render+install+start logic
//!   `crates/txtodo-cli/src/commands/service.rs` already had fully built for `txtodo daemon
//!   install|start`, moved here so [`ensure_daemon`] can also call it, best-effort, right after a
//!   successful ad-hoc spawn — the two problems this task fixes ("not running right now" and
//!   "won't come back after reboot") share one machine-local piece of state (the rendered service
//!   file), so one crate owns rendering it and either caller (a human's `txtodo daemon install`,
//!   or an ad-hoc spawn's own best-effort install) writes the same file.
//!
//! Zero dependency on `txtodo-daemon` (`.claude/budgets.json`'s `allowedDeps` keeps the
//! dependency direction the other way — the daemon depends on shared crates, not vice versa),
//! so `txtodo-mcp` and `txtodo-tui` (whose own `allowedDeps` lists are deliberately short) can
//! depend on this crate without pulling in the daemon's whole graph.
//!
//! Every client honors `TXTODO_NO_AUTOSTART=1` ([`autostart_disabled`]) as an opt-out, except
//! `apps/desktop`: a GUI app the user explicitly launched keeps its pre-existing always-spawn
//! behavior (task decision, `tasks/daemon-always-available/notes.md`). `TXTODO_NO_SERVICE=1`
//! ([`service_disabled`]) is the separate, narrower switch that keeps everything in this crate
//! off launchd/systemd; the repo's `.cargo/config.toml` sets it for tests.
#![forbid(unsafe_code)]

mod autostart;
mod binary_path;
pub mod service;
mod spawn;

pub use autostart::{autostart_disabled, service_disabled};
pub use binary_path::default_global_socket;
pub use spawn::{LaunchConfig, LaunchError, ensure_daemon};
