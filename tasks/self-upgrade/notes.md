# Upgrade to the latest release from the CLI, TUI and desktop

## Goal
- One way to move a device to the newest release from inside txtodo, not by remembering the
  install line. Asked by the human 2026-10-11.

## Today
- Three install paths (README): the curl script (`scripts/install.sh`: desktop app + four
  binaries in `~/.local/bin`, always the newest release), Homebrew (`aaronmyatt/tap/txtodo`, CLI
  only, can lag), and `just install` from a checkout (writes `<data dir>/txtodo/installed-build`).
- The daemon is a launchd job on `~/.cargo/bin/txtodod` or the installed path; an upgrade must
  restart it, or the old daemon keeps serving.

## Design
- `txtodo upgrade [--check]` does the work; TUI and desktop only call it.
  - Latest: GitHub's `releases/latest` (one HTTPS GET, only when asked: no background
    phone-home). Compare with `buildinfo::VERSION`.
  - How this copy was installed, from the running binary's path: under `brew --prefix` → `brew
    upgrade aaronmyatt/tap/txtodo`; `installed-build` present → a source build, say so and stop
    (run `just install`); else → the curl script with `TXTODO_VERSION=<tag>`.
  - Then restart the daemon (`txtodo daemon install --force`, which rewrites and kickstarts the
    unit) so the new `txtodod` serves.
  - `--check` only prints current vs latest and the command it would run. Never runs without
    the human asking.
- TUI: Settings > About row "v0.0.22 · check for updates"; Enter runs `txtodo upgrade --check`,
  a second Enter runs the upgrade after leaving the alternate screen, then quits.
- Desktop: Settings > General "Check for updates" button; runs the bundled `txtodo upgrade
  --check`, then "Upgrade" runs it and offers a relaunch.
  Ref: https://v2.tauri.app/develop/sidecar/

## Rejected
- tauri-plugin-updater: needs an update manifest and signing keys in the release pipeline, and
  only updates the app, not the CLI and daemon beside it.
- Checking for updates on every CLI run: a network call nobody asked for.

## Open questions
- None blocking. Linux: the curl script is macOS-only, so a non-brew Linux install gets the
  release URL to fetch by hand.
