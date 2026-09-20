# Working a txtodo backlog

A txtodo backlog is plain todo.txt: a root `todo.txt`, plus one `tasks/<slug>/{todo.txt,notes.md}`
per multi-step feature, linked from the root line via a `ref:<slug>` tag. Refs nest: a line inside
`tasks/<slug>/todo.txt` can itself carry `ref:<sub-slug>`, giving `tasks/<slug>/<sub-slug>/`.

Work it through the txtodo MCP tools (`todo_list`, `todo_search`, `todo_get`, `todo_add`,
`todo_complete`, `todo_uncomplete`, `todo_edit`, `todo_move`, `todo_delete`, `todo_archive`,
`todo_batch`, `todo_history`, `todo_raw`, `todo_notes_get`, `todo_notes_set`) or the equivalent
`txtodo` CLI commands — never by hand-editing the files while a daemon owns them.

## 1. Read the backlog

- `todo_list` the root `todo.txt` in full before touching anything. Every `todo.txt` under the
  workspace is also an MCP resource (`todotxt://...`), so enumerate the whole `ref:` tree
  (root → `tasks/*/todo.txt` → any nested `ref:`) rather than discovering it lazily mid-loop.
- Build a mental (or scratch) map of: which top-level lines have a `ref:` sub-backlog, which are
  already `x` done, which carry `@human` (a decision only a human makes), and which are already
  claimed (`@doing`).

## 2. Flesh out sub-todos and notes

Before looping, thin or missing detail is a blocker, not something to improvise mid-task:

- For each unclaimed, non-`x`, non-`@human` line with a `ref:` whose `tasks/<slug>/todo.txt` is
  missing, empty, or clearly just a restatement of the parent line, break it down into real
  sub-steps with `todo_add`/`todo_batch` before executing any of them.
- `todo_notes_get` the ref's `notes.md`. If it has no design context (approach, rejected
  alternatives, known gaps, open questions), write a short plan into it with `todo_notes_set`
  before starting work — future sessions (and later loop passes) read this first.
- A line with no `ref:` that's clearly multi-step is a candidate to *gain* one: creating the ref
  dir is lazy (first `todo_notes_set` or `todo_add` into it creates the directory), so don't
  hesitate to add one rather than cramming a plan into the root line's text.
- Skip this pass entirely for lines that are already well-specified (a `ref:` whose todo.txt
  already has concrete, actionable sub-lines) — don't rewrite work that's already planned.
- **Once a line carries `ref:`, its own text is a pointer, not the spec.** Per
  `txtodo-design.md` §2.6, "a task that needs more than one line gets a directory, not a richer
  line" — that rule applies to length, not just syntax. A root/parent line should read as one
  short clause (what + rough where); the brief, rationale, constraints ("leave X alone because
  Y"), and any sub-spec (a sink matrix, a schema, an API shape) belong in `tasks/<slug>/notes.md`
  as a `## Goal`/`## Design` section, with each independently-doable piece as its own line in
  `tasks/<slug>/todo.txt` — not folded into one paragraph on the parent line. If a task arrives
  as a long pasted brief, treat splitting it this way as part of step 2, not an optional tidy-up.
  Bad: a 40-word parent line reciting file:line targets, a "leave println! alone" caveat, and a
  sink-matrix spec inline. Good: parent line = `txtodo-cli: cli.command span + eprintln! cleanup
  ref:logging-cli`; the caveat, targets, and sink matrix live in `tasks/logging-cli/notes.md`;
  "add cli.command span", "convert 3 eprintln!s", "wire sink matrix" are separate
  `tasks/logging-cli/todo.txt` lines.

## 3. Loop top to bottom

Drive this phase with `/loop` (or an equivalent repeat-until-done driver). Traversal order is
**file order**, not priority or milestone — top to bottom, depth-first into any `ref:` subtree
before moving on to the next top-level line:

1. Find the first line, in file order, that is not `x`, not `@human`, and not already claimed
   (`@doing`) by someone else.
2. If it has a `ref:`, recurse into `tasks/<slug>/todo.txt` and fully clear that sub-backlog
   (steps below, applied inside that file) before returning to close the parent line.
3. **Claim**: `todo_edit` (append) `@doing` plus `by:<your-name>`. This is the only guard against
   two agents picking the same line — there is no server-side lock, so treat a missing claim tag
   as free and always add yours before touching anything else. Remove both tags again on close or
   abandon.
4. **Work**: re-read `notes.md` (`todo_notes_get`) even if you wrote it in step 2 — state may have
   moved on. As you work, `todo_notes_set` to append what you decided and why, in the backlog's
   existing voice: short, plain, honest about what's still broken rather than padded with what
   went well. Big architectural calls (new wire formats, new crate boundaries, anything that would
   need an ADR by this project's convention) are not yours to decide alone — add `@human` with the
   open question and stop on that line; don't let it block siblings.
5. **Close**: `todo_complete` the line, then `todo_edit` (append) a one-line summary in the
   existing style — what shipped, and any known gap named honestly rather than left implicit, e.g.
   `— real device-to-device pairing works; known gap: no shared identity across LAN/relay yet`.
   If the line carries `ref:`, that summary is one clause pointing at the notes (`— see
   tasks/<slug>/notes.md`), never a re-enumeration of what each sub-line did — that's already
   recorded in the sub-backlog's own completed lines and in notes.md's `## As built` section.
   Drop the `@doing`/`by:` claim tags first if `todo_complete` doesn't already clear them.
6. If closing this line finished every task under a `ref:<slug>` sub-backlog, go close or update
   the *root* line pointing at it too — finishing everything inside `tasks/<slug>/todo.txt` does
   not do that for you automatically.
7. Move to the next line in file order and repeat from step 1.

### 3.1 Keep todo.txt's updated

Write through immediately at every state change (claim, note update, completion) — don't batch
edits to the end of a work session or the end of the loop. A crash or interruption mid-task should
leave the file honestly reflecting where things stand, not silently ahead of reality.

### 3.2 Done items sit at the bottom

Completing a line moves it to the bottom of its own file: `todo_complete` (MCP) and `txtodo do`
(CLI) both do it, once, at that moment. Active work stays at the top and history at the bottom,
without a second file, and with nothing for you to run. Two things follow:

- Line numbers change when you complete a line. Find a line again by its id or its text before you
  append to it; never reuse the number you completed.
- Do not archive after each completion. `todo_archive` / `txtodo archive` is the explicit full
  sort: it moves every done line down and drops blank lines, including done lines a human placed
  somewhere on purpose. Run it only when someone asks for that.

Older lines in this repo's own `todo.txt`/`tasks/*/todo.txt` predate this and are interleaved
chronologically rather than done-sorted; don't reflow them.

## 4. Stay current

Periodically pull the `triage_inbox` prompt (default `@inbox` context) for anything an agent
quarantined for review, and `plan_today` / `weekly_review` for what to pick up next.

## Installing this skill

`txtodo skill install [--only claude|agents]` renders this file into `~/.claude/skills/txtodo-backlog/SKILL.md`
(global Claude Code skill) and a marked, idempotent block in the project's `AGENTS.md`, so an agent
in any session picks up this playbook without it being copy-pasted per project. `txtodo doctor`
nudges toward running it when nothing has installed it yet.
