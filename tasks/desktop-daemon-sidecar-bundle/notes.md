# desktop-daemon-sidecar-bundle

## Reported

Today's "Daemon: dead" incident traced to `txtodod` not existing yet at `$PATH` resolution time in
a dev checkout — an easy fix there (`cargo build --workspace`). But `spawn_txtodod`
(`apps/desktop/src-tauri/src/daemon/spawn.rs:84-97`) has no fallback beyond a bare `Command::new
("txtodod")` `$PATH` lookup. Outside a dev checkout — a real packaged install — there is no
guarantee `txtodod` exists anywhere on the machine at all.

## Current state

- `apps/desktop/README.md:35` documents `txtodod` on `$PATH` (via `cargo build --workspace`) as a
  hard prerequisite — a dev-only instruction, not something a packaged app's installer satisfies.
- `npm run tauri build` (`apps/desktop/README.md:49`) produces a production bundle
  (`bundle.targets: "all"` in `tauri.conf.json`) with no `txtodod` sidecar wired in — checked, no
  `externalBin`/sidecar config exists today.
- Checked for existing coverage before filing this: `tasks/desktop-tauri-shell`,
  `tasks/brew-distribution`, `tasks/desktop-cask-distribution` — none mention bundling `txtodod`
  with the desktop app specifically. `desktop-cask-distribution`'s closed line (root `todo.txt`)
  covers a Homebrew Cask for `apps/desktop` (Tauri) alongside `txtodo`/`txtodod`/`txtodo-tui` "on
  every release bump" — **check `Casks/txtodo-desktop.rb` for a `depends_on` on the `txtodo` formula
  before assuming this is unhandled**; if the cask already pulls in `txtodod` as a dependency, a
  Homebrew-installed desktop app may already be covered and this task narrows to non-Homebrew
  distribution channels only (DMG-only sideload, etc).

## Open question for the human

Two different fixes, not mutually exclusive:
1. **Tauri sidecar**: bundle a `txtodod` binary inside the `.app`/bundle itself
   (`tauri.conf.json`'s `bundle.externalBin`), set `daemon_bin` to the sidecar path instead of
   relying on `$PATH`. Works regardless of install method, but means the desktop bundle now ships
   and must keep in sync with its own copy of `txtodod`.
2. **Rely on package-manager dependency**: if the Homebrew Cask already `depends_on` the `txtodo`
   formula (which installs `txtodod`), that may be sufficient for that one distribution channel —
   narrower fix, but leaves any other distribution channel (raw DMG, etc.) still broken.

Tagged `@human` for that reason — same bar this repo already uses for packaging/distribution calls.

## Acceptance (once decided)

- A machine with no Rust toolchain and no prior `txtodo` install, installing only the desktop app
  through the chosen channel, gets a working daemon on first launch — no manual `cargo build` step.
