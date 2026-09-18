# Desktop never assumes a workspace

## Reported
2026-09-19: a Finder-launched desktop app showed "Daemon: dead" on every launch. Its log:
`startup_connect_failed ... open /: discover documents: cannot list /.txtodo: Read-only file system`.
`lib.rs::workspace_dir()` defaulted to the launch cwd, which is `/` outside a shell; connect then
sent a `Path("/")` selector and the daemon tried to open `/` as a workspace.

## Principle (from the human)
The desktop app only reflects workspaces linked explicitly (add/switch) or touched by the cli/tui
(the daemon's registry). It never picks one itself.

## As built
- `DesktopConfig.workspace` and `AppState.current_workspace` are `Option<PathBuf>`; the only
  source is the explicit `TXTODO_WORKSPACE` override (dev/test). `DesktopConfig::unbound()` added.
- `connect_and_store` sends no selector while nothing is selected; `workspace_root` returns "".
- `DaemonClient::wait_until_ready` probes with the registry-level `workspace_list` when there is no
  selector: an unselected `Health` is refused while zero or several workspaces are open, which
  would have turned "no workspace" into "Daemon: dead" all over again.
- MainView shows a "No workspace selected" message instead of a file view until one is picked.
- Removed the dead per-workspace `socket_path()/state_dir()` helpers (ADR 0010 legacy).
- Tests: `an_unbound_client_is_ready_and_lists_an_empty_registry` (real daemon).

## Known gaps
- Not run: Playwright e2e/visual suites (they use a mock/bridge with a fixed workspace); the
  empty state has no visual golden. A human should launch the built app from Finder once.
- Per-workspace commands (list_files etc.) still go out with no selector when nothing is picked;
  the daemon resolves that to "the sole open workspace" or an ambiguity error the UI surfaces.
