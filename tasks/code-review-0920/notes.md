# code-review-0920

## Goal

Fix what the 2026-09-20 code review found in the day's commits (`ecbcf5b..bfabbcc`, 44 commits:
daemon early-bind, daemon-launch, MCP parity, desktop edit-loss fixes). One `todo.txt` line per
finding, worst first. The review read code only: no test was run and nothing was reproduced, so
each line starts with "prove it" (a failing test), then the fix.

## Findings

Numbered like the lines in `todo.txt`.

1. **launchd wait-forever** — `crates/txtodo-daemon-launch/src/service.rs:320` `is_loaded`.
   `launchctl print` succeeds for a job that is loaded but not running. A daemon that loses the pid
   lock now exits 0, and `KeepAlive.SuccessfulExit=false` never restarts it. Normal path, not an
   edge: ad-hoc daemon holds the lock, `install_persistent_service_best_effort` bootstraps the unit,
   the unit's daemon exits 0 and stays loaded. When the ad-hoc daemon later dies, every client takes
   the `already_installed_as_service` wait branch (`spawn.rs:124`), times out, never spawns.
   Fix idea: "loaded" on macOS must mean running (parse `state = running` / a pid from
   `launchctl print`), or kickstart (no `-k`) a loaded-but-stopped job instead of waiting.
2. **Remove during open** — `crates/txtodo-daemon/src/workspace_catalog_load.rs:71` `run_open`.
   The old code held the `open` write lock across the open, so `remove_registered` waited. Now a
   remove during Loading finds nothing, forgets the slot, and the open then inserts the workspace
   anyway: watcher, actors and routes stay live, and `resolve_sole_open` counts it.
   Fix idea: after the open, insert only if the slot still exists (or the registry row is active);
   else drop the `OpenedWorkspace`.
3. **Shutdown hang** — `crates/txtodo-daemon/src/global_service.rs:71`, `main.rs:160`.
   `resolve` parks on the blocking pool (Condvar wait up to `load_wait` 120 s, or a whole open).
   Dropping a plain `Runtime` waits for started blocking tasks. `run` has already dropped the pid
   lock and socket by then, so a new daemon can start while the old one is still opening a store.
   Fix idea: `rt.shutdown_timeout(..)` in `main`, and wake waiters on shutdown.
   Ref: https://docs.rs/tokio/latest/tokio/runtime/struct.Runtime.html#method.shutdown_timeout
4. **rejectedEdits keyed by path** — `apps/desktop/src/lib/components/FileView.svelte:226`,
   `stores/rejectedEdits.ts`. Every workspace has a `todo.txt`. The unmount commit on a workspace
   switch runs after Rust has switched the selector, so it is refused and parked under `todo.txt`;
   the next good save to the new workspace's `todo.txt` clears it. Key by workspace root + path.
5. **Client mutex across a 120 s call** — `apps/desktop/src-tauri/src/commands.rs:204` (and every
   `*_inner` that holds `state.client` across its RPC). A selector-scoped call on a Loading
   workspace now blocks in the daemon instead of failing fast. `list_workspaces` (the switcher's
   2 s poll) queues behind it, and "Opening this workspace…" never shows, because `Unavailable`
   only comes back after the timeout. Needs a design call, see Open questions.
6. **Retry bound** — `apps/desktop/src/lib/loadingRetry.ts:12`. 300 tries, each able to block
   ~120 s in the daemon: about 10 hours, not the "five minutes" the comment says. Use a deadline.
7. **MCP substring match** — `crates/txtodo-mcp/src/resources.rs:78`, `parse.rs:122`.
   `todotxt://project/work` now returns `+workshop`; `context/home` returns a line with
   `bob@home.com`; `triage_inbox` pulls `@inbox-old`. The old code matched `row.projects` /
   `row.contexts` exactly. Keep `matches_query` for `todo_list`/`todo_search` (CLI parity), match
   the token exactly for the three typed callers.
8. **`done` / `not done` gone** — `crates/txtodo-mcp/src/parse.rs:122`. They are plain substrings
   now, so `query: "not done +work"` returns almost nothing, silently. `TaskRow.done` still exists.
9. **Selector-less Health** — `crates/txtodo-daemon/src/global_service.rs:153`. Totals-only while
   loading, then `resolve_sole_open` fails with 0 or 2+ open workspaces ("ambiguous"); the pending
   check is repeated there, so it can also race to `Unavailable`. The totals are device-level:
   always answer totals-only when no selector is given.
10. **Vacuous assertion** — `crates/txtodo-daemon/tests/single_instance.rs:87`. It checks stderr
    lacks "registered workspace", but `start_global` and that line were deleted in the same change.
    Assert the seeded workspace has no `.txtodo/oplog.db` instead.
11. **Path fast path locks** — `crates/txtodo-daemon/src/workspace_catalog.rs:247`. One
    `canonicalize` plus a read lock on every open workspace per request, on a runtime worker. A
    held write lock on workspace A stalls a request for B. Keep the canonical root on
    `OpenedWorkspace` or in a root→id map.
12. **Health cost** — `crates/txtodo-daemon/src/workspace_catalog_load.rs:114` `load_totals`.
    `list_registered` = registry lock + SQLite list + two `stat`s per workspace, per Health call,
    in the async handler. A `COUNT(*)` or a kept count is enough.
13. **recency_ms** — `workspace_catalog_load.rs:18`. A month-old `last_active_ms` beats a
    `todo.txt` edited today. Use the max of the two.
14. **carries_own_tag** — `crates/txtodo-daemon/src/migrate_sidecar.rs:93`. `fast_id_of` returns
    the first `id:` word; the daemon appends the line's own tag last. A line reading
    `see id:<OTHER> … id:<OWN>` is never seen as tagged, so the document is skipped forever.
    Least sure of the 15: check what `fast_id_of` really returns first.
15. **Doc URLs** — `~/.claude/CLAUDE.md` §1 asks for a doc URL wherever an API is used. Missing on
    `tokio::runtime::Handle::current()`/`enter` (`workspace_catalog_load.rs:165`),
    `spawn_blocking` (`global_service.rs:71`), `JoinHandle::abort` (`state.rs`), `loadingRetry.ts`.

## Open questions

- Line 5 is a design call, not a patch. Option A: the daemon answers `Unavailable: workspace
  loading` at once for a Loading workspace when the client asks it to (a request flag), and the
  desktop retries. Option B: the desktop clones the tonic client per call so nothing queues behind
  a slow one. B is smaller and fixes the freeze; A is what makes the banner show.
- Lines 1 and 3 touch how the daemon starts and stops under launchd. Check by hand on macOS after
  the fix; the unit tests cannot see launchd.

## Slices

`@service` line 1 (txtodo-daemon-launch) · `@daemon` lines 2, 3, 9–14 · `@desktop` lines 4–6
(unfenced) · `@mcp` lines 7–8. One Cargo crate lease per agent session.

## Progress 2026-09-20 (second session)

Done, one commit each, test first where a test could show it: 1 (`2c52fa4`), 2 (`922f799`, the
test failed before the fix), 3 (`bb51e4c`), 4 (`958205f`), 5 (`7901016`, option B), 6 (`d2e3140`),
7 (`3f55049`), 8 (`d513ed4`, a `done` field on `todo_list`, not magic query words).

Stopped at 9: another agent session held an uncommitted change in this checkout, and the slice
fence will not hand over a second crate while the tree is dirty. What I read for the rest:

- **9** The fix idea in this file ("always totals only with no selector") would break callers.
  `tests/support/mod.rs::health()` and the `--dir` bridge send no selector and read
  workspace-level fields (LAN flags, relay outcome). Smaller fix: a selector-less Health never
  fails. Keep today's answer when exactly one workspace is open, and answer totals only when
  `scoped()` errors (0 or 2+ open, or the race to `Unavailable`). Harness:
  `workspace_catalog_state_tests.rs` already builds a `GlobalService` in-process.
  `global_service.rs` is at 394 of 400 lines.
- **13** `max(last_active_ms, mtime)` is right, but
  `a_workspace_a_request_used_loads_before_one_only_its_file_mtime_favours` will flip: its
  `FakeClock` starts at 1 000 ms (1970) and the file mtime is real time, so the mtime always wins.
  Give that test a `FakeClock` set near the real now.
- **14** Checked, as asked: `fast_id_of` does return the first `id:` word. But `id_strip.rs`
  defines "own tag" as exactly that first word and leaves a later tag alone on purpose, and in
  Tagged mode a line's identity is its first `id:` word, so "own tag is second" can only happen
  after a hand edit under Sidecar, where the tag is inert. Making `carries_own_tag` stricter
  without changing `strip_own_id` would make the migrate never finish. I would close 14 as "by
  design", with a test that pins it.
- **5, the rest** "Opening this workspace…" still shows only after the daemon's own wait. That is
  option A (a fail-fast flag on the request): a wire change, a human's call.

## As built, the rest (2026-09-20, after the other session committed)

- 9 `fix(daemon): a selector-less Health never fails`: narrower than the finding asked, see the
  Progress section for why.
- 10 `2ca6093`, 11 `a106db7`, 12 and 13 `cd0d889`, 15 `aa4d5c3` plus lines in the other commits.
- 14 closed with no change, by design (Progress section).
- Finding 11's test found one more thing: `OpenedWorkspace` kept the root as the caller spelled it,
  so on macOS (`/var` is a symlink) the path fast path never matched and every call took the
  blocking-pool slow path. The root is canonical now.

Left for a human: finding 5's "Opening this workspace" banner (option A, a wire flag), and a hand
check of findings 1 and 3 under real launchd (`txtodo daemon stop`, kill the ad-hoc daemon, see a
client spawn one; SIGTERM a daemon mid-open, see it exit within 5 s).
