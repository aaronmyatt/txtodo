# agent-skill-install-playbook

## Context

User asked for a SKILL.md so an agent works a txtodo backlog like:

1. Read the backlog (parent/root todo.txt)
2. Flesh out sub-todo.txt / notes.md
3. `/loop` top to bottom, always keeping todo.txt (and its `ref:`'d files) updated, and sorting
   `x ` done items to the bottom.

Inspiration cited: https://github.com/MrLesk/Backlog.md. Also wanted: `txtodo doctor` or a
dedicated `txtodo llm/agent/setup/skill` command offering to install the skill for the user.

## What was already in flight

A sibling, uncommitted worktree — `agent-txtodo-backlog-c45e46` (branch
`claude/agent-txtodo-backlog-c45e46`, not merged to `main`) — was already building almost this
exact thing, discovered via an Explore pass before writing anything here:

- `skills/txtodo-backlog.md` (v1, 51 lines) — a "pick the highest-priority unblocked task" playbook
  with pick/claim/work/close sections.
- `crates/txtodo-cli/src/commands/skill.rs` (new) + edits to `cli.rs`/`main.rs`/`commands/mod.rs` —
  implements `txtodo skill install [--only claude|agents]`, writing the playbook to
  `~/.claude/skills/txtodo-backlog/SKILL.md` and a marked idempotent block in the project's
  `AGENTS.md`, both rendered from the one canonical `skills/txtodo-backlog.md` via `include_str!`.
  This is exactly the "explicit CLI command to set up the skill" the user asked for.
  `--only claude|agents` are the only two targets — no Cursor/Copilot/Windsurf, by design.
- `crates/txtodo-cli/src/commands/doctor.rs` edit — an advisory (never-FAIL) row nudging toward
  `txtodo skill install` when nothing has installed it yet.
- `tasks/agent-skill-install/todo.txt` (their task) — 3 of 6 lines done; open items are a
  `txtodo-tui` status hint, an `apps/desktop` onboarding hint (both deferred to separate sessions,
  different stacks), and an `@human`-gated idea for a real atomic `todo_claim` MCP tool once the
  `@doing` tag convention proves too weak.
- Design rationale in their `notes.md`: canonical playbook lives at top-level `skills/`, not under
  `.claude/`, because this repo's own `.claude/**` is a frozen path per `budgets.json`.

User chose (asked via AskUserQuestion): **extend that playbook rather than write a competing one**.

## What changed in v2 (this task)

The v1 playbook's traversal model was "prefer the highest-priority, lowest-milestone, unblocked
task" — a picker, not a loop. It also had no pass for fleshing out thin sub-todos before working
them, and no explicit sort-to-bottom step. v2 (`skills/txtodo-backlog.md` in this worktree)
restructures it into four numbered phases matching the user's spec exactly:

1. Read the backlog — enumerate the whole `ref:` tree via `todo_list` + resources before acting.
2. Flesh out sub-todos and notes.md — a dedicated planning pass for any `ref:` that's thin, empty,
   or just restates the parent line; done *before* phase 3 touches it.
3. Loop top to bottom — strict file-order traversal (not priority-sorted), depth-first into a
   `ref:` subtree before advancing past its parent line. Kept the v1 claim/work/close mechanics
   (`@doing`/`by:`, `@human` gate, notes read-before-write) inside this phase rather than
   reinventing them — they're sound design regardless of pick-order.
   - 3.1 keep todo.txt updated: write through immediately per state change, no batching.
   - 3.2 sort `x` to bottom: `todo_archive`/`txtodo archive` immediately after each completion,
     per file — not deferred to end of loop, since traversal order depends on reading "first
     non-done line" again on the next pass.
4. Stay current — kept verbatim from v1 (`triage_inbox`/`plan_today`/`weekly_review` prompts).

Explicitly noted in the doc: sort-to-bottom is a convention this skill imposes *going forward*.
Confirmed via Explore that this repo's own root `todo.txt` and task-level files today interleave
`x` and active lines chronologically, not done-sorted — the skill does not ask agents to reflow
historical lines, only to apply the convention to lines they complete themselves.

## Known gap — not solved here, tracked above

This worktree only carries the doc (`skills/txtodo-backlog.md`). The actual `txtodo skill install`
command, `doctor` hint, and their wiring live only in the uncommitted sibling worktree/branch.
Nothing here re-implements that CLI plumbing — duplicating it in two worktrees against the same
untracked files would just create a merge conflict later for no benefit. See the open `@human`
line in this task's `todo.txt`: someone with repo-write access needs to decide how the two
branches converge (merge `claude/agent-txtodo-backlog-c45e46`, or cherry-pick `skill.rs` + the
wiring diffs onto this content) before `txtodo skill install` actually ships the v2 playbook.

## Reconciliation, done 2026-09-17

User asked to "merge the sibling worktree branch into this one." Checked first: `git log --oneline
main..claude/agent-txtodo-backlog-c45e46` was empty — that branch has zero commits ahead of `main`,
so a `git merge` would have been a no-op. The actual skill-install work was entirely uncommitted in
that worktree's working tree (`git status` there showed `skill.rs`/`skills/`/`tasks/agent-skill-install/`
as untracked, `cli.rs`/`main.rs`/`commands/mod.rs`/`doctor.rs` as modified-but-uncommitted).
Copied that diff across by hand instead of running a merge:

- `crates/txtodo-cli/src/commands/skill.rs` copied verbatim — its
  `include_str!("../../../../skills/txtodo-backlog.md")` already resolved to this worktree's v2
  playbook, no path change needed.
- `cli.rs` (new `Skill` variant), `commands/mod.rs` (`pub mod skill`), `doctor.rs` (`skill_check`,
  appended after `other_workspace_checks` so it doesn't disturb the `debug_assert_eq!(checks.len(),
  7, ...)` fixed-check-count assertion) applied 1:1 from the sibling's diff.
- `main.rs` needed hand-adaptation, not a straight copy: this worktree's `main.rs` already has a
  newer refactor the sibling's diff predates (`dispatch`/`dispatch_daemon` split into
  `dispatch_inner`/`dispatch_daemon_inner` for the `cli.command` tracing span, root todo
  `logging-cli`). Added `Command::Skill { action } => return commands::skill::run(action)` to the
  early-dispatch match, folded `Skill` into the existing `unreachable!` arm, added a
  `needs_daemon_err()` helper (this worktree didn't have one; the sibling's version inlined the
  message), and added `Command::Skill { .. } => "skill"` to `command_name` (a match this worktree's
  `main.rs` has that the sibling's version doesn't).
- Copied `tasks/agent-skill-install/{todo.txt,notes.md}` (their task tracking the install command
  itself) and their root `todo.txt` line (`ref:agent-skill-install`,
  `id:01M2HTC5Y6X222T7VZA4Y8M223`, still open — remaining sub-items are a `txtodo-tui` status hint
  and an `apps/desktop` onboarding hint, both correctly deferred to separate sessions per that
  task's own notes) so that backlog isn't stranded in the other, still-uncommitted worktree.

Verified for real, not just by inspection: `cargo check -p txtodo-cli` clean; `cargo test -p
txtodo-cli --bin txtodo skill` — 3/3 `commands::skill::tests::*` pass; a live round trip against a
scratch `$HOME` (`HOME=/tmp/... txtodo skill install --only claude` then `txtodo doctor`) wrote a
real `SKILL.md` containing this worktree's v2 content and flipped doctor's `skill` row from `warn`
to `ok`.

Not done here, still real: the sibling worktree/branch itself still has its own uncommitted copy of
these same files (a second copy, now diverged from this one — this worktree's `skill.rs`/`doctor.rs`
etc. are the ones with the fix-ups above). Nobody should commit from that sibling worktree without
first checking whether this worktree's version has already superseded it, or the two will produce
a real merge conflict later.

## Open questions surfaced, not answered here

- Should `todo_complete` clear `@doing`/`by:` tags itself, or does the agent still need to
  `todo_edit` them off separately? v1 and v2 both leave this to the agent to be safe; worth
  checking whether the MCP tool already does it before writing that step off as boilerplate.
- The user's original phase 3 (`/loop top to bottom`) had an unfinished "4." bullet before their
  message was interrupted and restated without it — nothing beyond sort-to-bottom was ever
  specified, so v2 stops there rather than guessing a fourth loop sub-step.
