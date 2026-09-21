# layout-doc-drift

Found reviewing the last 100 commits (`18b37ea^..HEAD`) on 2026-09-21.

## Goal

ADR 0029 (default workspace) and ADR 0030 (workspace layout) landed and `specs/ref-directories.md`
plus plan §3.2 were updated with them. The human-facing docs were not, and now contradict the
normative ones.

## The drift

- `txtodo-design.md:150` (§2.6) still says `ref:<slug>` names a directory "beside the file
  containing the line", with the example `~/todo/todo.txt` + `ref:q4-roadmap` → `~/todo/q4-roadmap/`.
  ADR 0030 superseded that. §2.6 never mentions `txtodo.toml`, `refs_dir` or `todo_file` at all.
  This is the doc a human reads first, and it now directly contradicts the spec.
- `README.md:101` (and `:30`): "Resolved in this order: `--dir` > `$TXTODO_TODO_DIR` > config
  `todo_dir` > current directory." ADR 0029 replaced the tail: with no `--dir` it is now
  cwd-*if-it-is-a-workspace*, else the default workspace (`choose_workspace`), and the client
  announces which. A user reading README will not know where `txtodo add` landed.
- `README.md:99-110,87`: the Configuration section shows only `txtodo/config.toml`, not the new
  per-workspace `<root>/txtodo.toml`, and the command table row still reads
  `workspace [list|add|remove ID]` — no `layout`, `default` or `prune`. There is no README path to
  discovering the layout at all.
- `CLAUDE.md:88-98` (§3.2) still says "After completing a line, archive that file … Do this
  immediately per completion". `skills/txtodo-backlog.md:90-99` and `AGENTS.md:98` now say the
  opposite: "Do not archive after each completion. … Run it only when someone asks." CLAUDE.md is
  the highest-precedence instruction file for every Claude Code session in this repo, so agents
  keep doing the thing the skill just banned. Worse, `txtodo skill install` writes only
  `~/.claude/skills/.../SKILL.md` and `AGENTS.md`
  (`crates/txtodo-cli/src/commands/skill.rs:49,61`), so CLAUDE.md's marked `txtodo:skill` block can
  never be refreshed by the tool and will drift again. Either add CLAUDE.md as a third install
  target, or delete the block and point it at AGENTS.md. The gap is noted in
  `tasks/complete-to-bottom/todo.txt:1` with no follow-up.
- `tasks/workspace-layout/todo.txt:16,18` — two closed lines give "the daemon refuses any other
  todo_file" as the reason ROOT_PATH stays `todo.txt`. That stopped being true in the same range
  (`9d28fd9`, `f99bf0d`). A closed line's summary is what the next reader trusts; a false reason
  there is worse than no reason.

The `--todo-file` clap help says the same false thing; that one is filed under
[[layout-client-gaps]] since it is a code change.

## Not in scope

Not proposing a docs rewrite. Each bullet is a specific stale sentence with a file:line — fix those
and stop.
