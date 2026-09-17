# agent-skill-install

Backlog.md-inspired: a playbook that lets an agent pick, claim, work and close a txtodo task on
its own via the existing MCP tools/prompts (`txtodo-mcp` already had the full surface — the gap
was procedure, not capability).

Design decisions:

- Canonical content lives at top-level `skills/txtodo-backlog.md`, not under `.claude/`: this
  repo's own `.claude/**` is a frozen path (`budgets.json.slices.frozenPaths`), so a project-local
  Claude Skill can't be authored directly here. `txtodo skill install` writes the *user's* real
  global `~/.claude/skills/txtodo-backlog/SKILL.md` instead — which is what "encourage adding the
  skill globally" actually meant anyway.
- Two install targets only: `claude` (global Skill) and `agents` (this project's `AGENTS.md`, the
  one cross-agent project-root convention with real multi-tool uptake today). Didn't fabricate
  support for Cursor/Copilot/Windsurf's own proprietary global-config formats.
- Ownership claiming uses a plain `@doing`/`by:<name>` tag pair, not a new `todo_claim` MCP tool —
  a daemon `Apply` change is architecture this project gates behind an ADR + human sign-off
  (see `@human` items throughout root `todo.txt`); a tag convention needed nothing new.
- Scope cut for this session: `crates/txtodo-cli` only. The fence is one leased crate per session
  (`.claude/hooks/fence.sh`), so the TUI hint (`txtodo-tui`, its own crate) and the desktop hint
  (`apps/desktop`, a different stack — SvelteKit/Tauri) are separate sub-tasks for separate
  sessions, not skipped.
- 2026-09-17: TUI hint done, this session, `txtodo-tui` leased. Couldn't share `txtodo-cli`'s
  `claude_installed()` (no `allowedDeps` edge tui->cli), so the one-file SKILL.md check is
  duplicated in a new `crates/txtodo-tui/src/skill_hint.rs`. Still open: the `apps/desktop` hint
  (its own session — different stack, no fence edge to claim, but the design cut above still
  applies) and the `@human`-gated `todo_claim` idea.
