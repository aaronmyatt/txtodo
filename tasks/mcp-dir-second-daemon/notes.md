# mcp-dir-second-daemon

## Goal

On 2026-09-28 at 23:36 +0800, `txtodo mcp --stdio` run in `/Users/oya/Development/txtodo`
started `txtodod --dir /Users/oya/Development/txtodo` next to the global launchd daemon, which
already had that root as workspace 01M3CDSTQRW6H5RWC0ZMKA9G8X. The two shared
`.txtodo/oplog.db` and every file:

- MCP writes went to the `--dir` daemon (its own identity, ops signed 01A0B561…); the global
  daemon's watcher took each write as an external edit and reconciled it again, so every delete
  landed twice in the log (320 ops from each).
- The `--dir` daemon bound the same relay endpoint id; the relay dropped the global one until
  the `--dir` one stopped.
- `txtodo workspace list` in that folder talked to the `--dir` daemon (the CLI prefers a live
  per-folder socket), which lists the root under its own per-folder registry id.

## Cause

`txtodo mcp` (`crates/txtodo-cli/src/commands/mcp.rs`) always execs `txtodo-mcp --dir <dir>`.
`txtodo-mcp` with `--dir` dials `<dir>/.txtodo/txtodod.sock` and, when nothing answers there,
`ensure_daemon` spawns `txtodod --dir <dir>` (`LaunchConfig::with_dir`). Nothing asked whether
the device's global daemon already owns `<dir>`. The `--dir` bridge itself uses the per-folder
registry `<dir>/.txtodo/registry.db`, so its own overlap check never saw the global one.

## Design

Two layers:

1. `txtodo-mcp --dir <dir>`: before anything else, probe the global socket. If a daemon answers
   there and one of its workspaces is `<dir>` or holds it, serve through that daemon, aimed at that
   root, and say so on stderr. Only a live daemon is asked: no autostart and no upgrade here, so a
   `--dir` run against a tmp folder (every harness) keeps its own bridge. The CLI needs no change:
   it still passes `--dir`, and now lands on the global daemon for a registered folder.
2. `txtodod --dir <dir>`: refuse to start when the global registry (`registry_db_path`, which
   honours `$TXTODO_REGISTRY_DB`) holds an active root equal to, inside or around `<dir>`, unless
   that registry is the bridge's own. Checked before the pid lock and `.txtodo/` are touched. This
   also covers a global daemon that is not running when the bridge would start.

## Known gaps

- A stale `<dir>/.txtodo/txtodod.sock` file (a bridge that crashed) still wins the CLI's socket
  choice and then fails to connect; not touched here.
