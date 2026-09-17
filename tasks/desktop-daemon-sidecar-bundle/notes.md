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

## Decision (2026-09-18, per the overnight task brief that authorized this build)

Do both, not mutually exclusive — matches the brief's own framing:

1. **Primary: Tauri sidecar.** `tauri.conf.json`'s `bundle.externalBin: ["binaries/txtodod"]`.
2. **Defense in depth: Homebrew Cask dependency.** `depends_on formula: "txtodo"` on the cask.

## As built (2026-09-18, agent)

- **`apps/desktop/src-tauri/tauri.conf.json`**: added `bundle.externalBin: ["binaries/txtodod"]`.
  Tauri's own convention (confirmed against the real v2 docs, `https://v2.tauri.app/develop/
  sidecar/`, fetched during this session): stage target-triple-suffixed binaries at
  `src-tauri/binaries/txtodod-<target-triple>[.exe]` before `tauri build`; the bundler copies the
  right one in and strips the suffix for the platform actually being built.
- **`apps/desktop/src-tauri/src/config.rs::sidecar_daemon_bin`/`sidecar_candidate`** (new,
  `DesktopConfig::new` now calls it): resolves the sidecar as "beside this process' own
  executable, named `txtodod`" — the same "beside the binary, else PATH" convention
  `crates/txtodo-cli/src/commands/service.rs::txtodod_path` already used for the CLI's own
  sibling `txtodod`, not the `tauri-plugin-shell`/`app.shell().sidecar(...)` API the docs also
  describe. **Why not `tauri-plugin-shell`**: that API is designed around its own `Command`/
  `CommandChild`/event-stream spawn model, which doesn't fit `ensure_daemon`'s existing
  probe/lock/spawn/wait sequence (shared with every other client via `txtodo-daemon-launch`,
  `task daemon-always-available`) without either forking that logic just for desktop or adding a
  `tauri-plugin-shell` dependency (plus its own capabilities/permissions JSON wiring) purely to
  resolve a path. The sibling-executable convention is how Tauri's bundler actually lays out
  `externalBin` files on every platform this app ships for (same directory as the main
  executable — verified against the fetched docs' `resourceDir()` description and cross-checked:
  it is *not* the resource dir, which is a different location on macOS specifically,
  `Contents/Resources` vs `Contents/MacOS`) — this repo's own established convention for the same
  problem, reused rather than re-solved with a new dependency. **This exact resolution has not
  been verified against a real, built `.app` bundle** — this sandbox has no GUI/bundler to
  produce or open one. A human should build a real package (`npm run tauri build` after `just
  stage-desktop-sidecar`, or via a real CI release run) and confirm the sidecar is actually found
  and spawned, not just that the unit test below is internally consistent.
- **`justfile::stage-desktop-sidecar`** (new recipe): builds `txtodod` for the host's own target
  triple (`rustc --print host-tuple`) and copies it into `src-tauri/binaries/` with the expected
  name, for local `npm run tauri build` testing. Ran once in this session: builds and stages
  `txtodod-aarch64-apple-darwin` successfully (confirmed the built artifact exists at the
  expected path — did not proceed to an actual `tauri build`/bundle, no GUI in this sandbox).
- **`.github/workflows/release.yml`**'s `build-desktop` job: new step "build + stage txtodod as
  this leg's Tauri sidecar" between `npm ci` and `tauri build`, building `txtodo-daemon --bin
  txtodod` for the exact same `--target` the leg's own `tauri build --target` call uses, staged
  at `apps/desktop/src-tauri/binaries/txtodod-${{ matrix.target }}`. **Not verified by a real CI
  run** — YAML syntax checked locally (`python3 -c "import yaml; yaml.safe_load(...)"`, passes),
  but the actual GitHub Actions execution (does the cross-target `cargo build` succeed on
  `macos-latest` for both the native and `x86_64-apple-darwin` cross leg, does `tauri build`
  actually pick up the staged binary and produce a bundle with a working daemon) is untested —
  only a real tag-triggered release run proves this end to end. Flagging plainly per this task's
  own instruction: this cannot be fully verified in this sandbox.
- **`apps/desktop/src-tauri/.gitignore`**: `/binaries/*` gitignored (regenerated per build),
  `!/binaries/.gitkeep` keeps the directory itself present in git.
- **`deploy/homebrew/Casks/txtodo-desktop.rb`**: added `depends_on formula: "txtodo"` (same-tap
  reference — both cask and formula are staged for `aaronmyatt/homebrew-tap`) as defense in
  depth, so a Homebrew-channel install gets a working `txtodod` on `$PATH` even if the sidecar
  resolution above ever regresses.
- **`apps/desktop/README.md`**: rewrote the `txtodod` requirement bullet to describe all three
  resolution paths in the order they're actually tried, and pointed at the new `just` recipe.

## Test coverage

- `apps/desktop/src-tauri/src/config.rs`: `sidecar_candidate_is_named_txtodod_beside_the_given_executable`
  (pure naming logic, no filesystem) and `sidecar_daemon_bin_is_none_when_no_sibling_binary_exists`
  (a real, always-true negative case against the actual test binary's own directory — `cargo test`
  binaries live in `target/debug/deps/`, which never has a `txtodod` beside it). Both pass;
  `cargo test -p desktop` (full suite, 17 tests) still green throughout.
- **Not covered, and not coverable here**: whether a real packaged bundle's sidecar is actually
  found and spawned end to end. That needs a real `tauri build` output and a machine to install
  it on — flagged above and in the root todo.txt line's closing summary.
