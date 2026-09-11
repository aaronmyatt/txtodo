# deploy/launchd and deploy/systemd files and txtodo daemon install|start|stop|status (plan M3)

Design §5 table: macOS runs a launchd user agent, Linux a `systemd --user` unit. Plan §2 puts the
files under `deploy/`. Ref: launchd.plist(5) https://keith.github.io/xcode-man-pages/launchd.plist.5.html ·
systemd.service https://www.freedesktop.org/software/systemd/man/latest/systemd.service.html ·
`launchctl bootstrap` https://keith.github.io/xcode-man-pages/launchctl.1.html

## Templates
Plain text with two placeholders, `{{TXTODOD}}` and `{{WORKSPACE}}`, replaced by the CLI with a
bounded `str::replace` (no template engine). The committed files in `deploy/` are the templates;
the rendered copy lands in the user's agent directory. Label `com.txtodo.txtodod.<hash8>` where
`hash8` is the first 8 hex of blake3(workspace) so two workspaces can run two daemons.

## The txtodod binary
```
txtodod --dir <workspace>        # required; no cwd guessing in a service
```
Startup order: lock pid file → open store → walk → spawn actors → watcher → gRPC → log "ready".
Shutdown on SIGTERM/SIGINT (tokio::signal): stop accepting, drain actors, remove socket and pid.
A stale socket at startup (pid file unlocked) is removed with an info event.

## CLI subcommand
`txtodo daemon <install|start|stop|status>` shells out to `launchctl`/`systemctl` through
`std::process::Command` with fixed argv (no shell string). Exit codes are checked and surfaced with
the command line that was run. `status` prints: service state, socket path and whether it answers
`Health`, pid.

## Not in M3
Windows service (per-user service or startup task, design §5) and named pipes — both M10 with
`txtodo-tui`'s Windows work.
