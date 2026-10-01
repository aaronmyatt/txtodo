# lab-watch

## Goal

Run the P2P lab against each commit on main after it lands, in the background, without slowing
the work in the main checkout, and say only when something changed: a scenario that passed now
fails (with the commit that broke it), or one that failed now passes.

## Design

- **Queue.** A post-commit hook appends the commit to `$LAB_HOME/watch/queue` when it is on
  `main` and touches `crates/`, `deploy/lab/` or `scripts/lab/`. It does nothing else, so a
  commit costs nothing.
- **Worker.** `lab.sh watch` starts one background worker (`watch-stop` stops it and its run).
  It takes the newest queued commit and drops the rest (newest wins), checks it out in one
  reused detached worktree at `$LAB_HOME/watch/wt`, and runs the lab with `LAB_SOURCE` pointing
  there: the main checkout's harness tests that commit's source. Verdicts go to
  `$LAB_HOME/watch/verdicts.tsv`, keyed by commit.
- **The worktree sits outside the repo.** The host daemon walks the main workspace; a worktree
  under the repo root gets its lists registered and `id:`-stamped (see
  `feedback_txtodo_cwd_autoregister`).
- **Comparable runs.** One fixed seed (`LAB_WATCH_SEED`), no re-runs (`LAB_RERUN=0`), no filing
  from the run itself (`LAB_NO_TASK=1`), `nice`d.
- **Alerts.** Against the last tested commit: pass → fail is re-run once on the same commit (a
  flake is logged, not filed); a fail that holds is bisected over the commits in between (only
  that scenario), then filed as one `lab:<scenario>` line naming the first bad commit, plus a
  notification. Fail → pass is a notification.
- **Iteration speed.** The image builds inside Docker with shared cache mounts, never touching
  the host's `target/`; one run at a time (the lab lock; the worker waits for a manual run);
  Docker Desktop's CPU cap is the human's knob.

Rejected: a worktree per commit (disk and setup per commit for no gain: one checkout moved with
`git checkout --detach` is enough); testing every commit (a full run is 40+ min while scenarios
fail; newest-wins plus bisect finds the same first bad commit).

## As built (2026-10-01)

- `scripts/lab/watch.sh start|stop|status|queue [commit]|once`, functions in
  `scripts/lab/lib/watch.sh`. `LAB_SOURCE` (`lib/common.sh`) points the harness at another
  checkout; the worker sets it to its worktree, so the main checkout's current scenarios test the
  queued commit's source, and a report's "source:" names that commit (in the main checkout it
  read `HEAD` at report time, which moved during a long run).
- Verdicts: `$LAB_HOME/watch/verdicts.tsv` (when, commit, kind full|confirm|bisect, run,
  scenario, verdict, report). Only full runs set "last tested"; confirm and bisect runs test one
  scenario. Bisect walks only commits that touch the lab's inputs (`LAB_BUILD_INPUTS`,
  `scripts/lab`, `deploy/lab`). A regression is filed through the lab's own `file_task` (same
  template, verdict `regressed`, so an open `lab:<scenario>` line is not filed twice).
- `watch.sh` ends with `main "$@"; exit $?` on one line: bash reads a script as it runs, and
  editing `lab.sh` while a run was going could make it read from the middle of the new text.
  `lab.sh` itself still has the old shape (it was running when this landed).
- `stop` kills the worker with every descendant, collected first: killing a run's pid alone left
  its scenario shell running (seen 2026-10-01).
- `.githooks/post-commit`: on `main` only, `watch.sh queue HEAD`, about 0.1 s, never fails.
- `scripts/lab/test/watch_test.sh`: the logic with a fake runner and a scratch repo, no Docker,
  4 s. Known gap: not yet run against Docker; the first real run is the worker's first commit.
