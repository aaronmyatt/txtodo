# nextest-adoption

Found reviewing the last 100 commits (`18b37ea^..HEAD`) on 2026-09-21.

## Goal

`34edd73` pointed `just test` and the agent gate at `cargo nextest`. Nothing else moved with it, so
the repo now has a test runner that is required, uninstalled, undocumented, and not what CI uses.

## The three loose ends

- Nothing installs it. `.claude/setup-state.json:13` explicitly records "cargo-nextest … skipped".
  A fresh clone runs `just test` (`justfile:17-21`) and gets "no such subcommand".
- `docs/testing-guide.md:9-13,38-41` — the one doc a contributor reads — teaches `cargo test`
  exclusively. Its `-- --ignored` and `--test <file>` recipes do not translate to nextest syntax
  either, so following the guide under the new `just test` fails twice over.
- Every CI job still runs `cargo test` (`.github/workflows/ci.yml:70-87,117`). Local and CI now use
  different runners: nextest is process-per-test, `cargo test` is threads-in-one-process. A test
  that depends on process-global state passes in one and fails in the other — and this repo has
  plenty of global state (sockets, env, the registry).

`.claude/budgets.json:130` was updated to match the new command, so the gate enforces it.

## Decision

Either adopt it properly (install in setup + CI, rewrite the guide's recipes, switch the CI jobs) or
revert `just test` to `cargo test` and leave nextest as an opt-in local nicety. Adopting it is the
better end state — process isolation is worth real money in a repo this concurrent — but the
half-state we are in now is worse than either.

`tasks/dev-iteration-speed/notes.md` and `tasks/rust-build-speed/notes.md` carry the wider
iteration-speed context; read those first.

## As built (2026-09-23)

Adopted rather than reverted. `just test` fails loud with the install command when nextest is
missing; `just install-nextest` installs it; the guide's recipes are nextest; CI installs it via
`taiki-e/install-action@nextest` and runs `cargo nextest run` everywhere (`--run-ignored
ignored-only` for the CI-only real-daemon step). Not verified: a green CI run with the new steps —
push and watch the first one. `.claude/setup-state.json` still records nextest as "skipped"; that
file is the human's setup record and was left alone.
