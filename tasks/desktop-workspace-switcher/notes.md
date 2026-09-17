# desktop-workspace-switcher

## Summary

Desktop: workspace switcher talks to the one global daemon instead of spawning/dialing a per-cwd
daemon.

## As built

`DaemonClient` carries a `WorkspaceSelector` per RPC plus `switch_workspace(&mut self)` retargets
with no reconnect (backend `0f9631a`); top-nav `WorkspaceSwitcher.svelte` lists/adds/removes/
switches, `MainView` keys `FileView`/`ConflictBanner` on the active root so a switch remounts
against the new selector (frontend `8dea8d1`). `cargo test`/`clippy`/`fmt -p desktop` and
`npm run check`/`build`/`vitest` all green.
