# version-info

## Goal

Every txtodo program shows its version and release date, small, and the desktop app says so when
the daemon it talks to is a different build. Trigger, 2026-09-20: the installed app spawned its own
bundled `txtodod` from before early bind, and nothing showed that it was old.

## Design

- One format: `0.0.2 (2026-09-20)` in `--version`, `v0.0.2 · 2026-09-20` in a UI.
- Version already exists: Cargo workspace `version`, read by clap in `txtodo-cli/src/cli.rs`, by
  `txtodo-tui/src/main.rs` and by `Health.version` in `txtodo-daemon/src/health_grpc.rs`.
- Release date does not exist yet. A build script sets it: `$TXTODO_RELEASE_DATE` (the release
  workflow sets it from the tag), else `git show -s --format=%cs HEAD`, else `unknown`.
- The file is shared without a new crate: `build-support/buildinfo.rs`, included with `#[path]` by
  the build.rs of txtodo-cli, txtodo-tui, txtodo-daemon and apps/desktop/src-tauri. A new crate
  needs an ADR and an `allowedDeps` change. If `check-boundaries.sh` refuses the `#[path]`, stop
  and ask.
- Test the date fallback order by including the same file from a test in txtodo-cli.
- Unobtrusive: muted, one line, never a banner. The CLI prints it only in `--version` and
  `doctor`; the TUI drops it first when the terminal is narrow.
- Mismatch: compare the app's own version and date with `Health.version` and
  `Health.release_date`. Any difference warns and names both. A daemon that sends no
  `release_date` is an older build, so it warns too. That is the case that started this.
- The fix the warning points at: reinstall the app (it spawns its bundled sidecar) or
  `txtodo daemon install` then `start`.
- Desktop reads its own date through a Tauri command; `getVersion()` from the Tauri API gives the
  version only.
- `tauri.conf.json` and `apps/desktop/package.json` repeat the Cargo version by hand. The last line
  in the backlog makes them fail the gate when they drift.

## Rejected

- Git hash in the string: more noise, and version plus date was the ask.
- Build time as the date: the release workflow hash-matches two builds of the same tag, and a
  timestamp would make them differ.
- Date in `txtodo list` output: it is todo.sh-compatible and must not gain lines.

## Known gaps

- A dev build shows the last commit's date, and a dirty tree looks the same as the commit.
- Equal version and date with different code (two builds of one commit) is not caught.

## As built (2026-09-20)

- `build-support/buildinfo.rs` (`4e3cc6c`): included with `#[path]` by the build scripts of
  txtodo-cli, txtodo-tui, txtodo-daemon and apps/desktop/src-tauri. `check-boundaries.sh` accepts
  it. It reruns when `$TXTODO_RELEASE_DATE` changes or when HEAD moves (it asks git for
  `logs/HEAD`, so a worktree works). The fallback order is tested from txtodo-cli.
- `--version`: `txtodo 0.0.2 (2026-09-20)`, `txtodo-tui ...`, `txtodod ...`. A build with
  `TXTODO_RELEASE_DATE=2026-01-02` showed that date.
- TUI (`afec5a5`): the dim label at the right edge of the status line, only when the line fits.
- proto `7de7c0c`/`c343a8f`: `HealthResponse.release_date` (21). Daemon: `Health` (full and
  totals-only) and the two boot log lines.
- Desktop (`323dea2`, `81056e9`): `build_info` command, `VersionInfo.svelte` (label in the top bar
  and on Help, banner with the other banners), `versionInfo.ts::daemonMismatch`.
- `txtodo doctor`: first line is the version line; a `version` row after the fixed seven warns on
  a mismatch.
- `scripts/check-version-sync.sh`: passes here, fails on a drifted copy.

Left for a human, as `gate.patch` beside this file (`git apply tasks/version-info/gate.patch`):
`release.yml` sets `TXTODO_RELEASE_DATE` from the tagged commit in the build and build-desktop
jobs and the smoke test fails a binary whose `--version` lacks it; `justfile` runs the version-sync
script in `boundaries`. Both are frozen paths.

Not proven: nobody has looked at the label or the banner in the app or a real terminal, and the
banner is computed once at mount.
