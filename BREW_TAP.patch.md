# BREW_TAP.patch.md — status and remaining steps

Backlog item `id:01M2Q1BREWDISTRIBUTION01` (`brew-distribution`, plan M10). Unlike this repo's
other `*.patch.md` handoffs (`RELEASE_CI.patch.md`, `RELAY_CONVERGE_CI.patch.md`), the reason parts
of this needed a human wasn't a frozen path — a Homebrew tap has to live in its **own** GitHub repo
(`brew tap <user>/<name>` expects a repo literally named `homebrew-<name>`), which an agent has no
filesystem access to create by itself. The human (`aaronmyatt`) asked for the repo to be created
directly, so steps 1-2 below are done, not staged.

## Correction to the root `todo.txt` line's own assumption

That line says the tap repo is `txtodo/homebrew-tap`. There is no `txtodo` GitHub org — this
project's real identity is the user `aaronmyatt` (`gh repo view` confirms `aaronmyatt/txtodo`).
Everything below uses `aaronmyatt/homebrew-tap` (`brew tap aaronmyatt/tap`), Homebrew's own
single-tap-per-user convention (<https://docs.brew.sh/How-to-Create-and-Maintain-a-Tap>).

## Done

1. **Tap repo created and seeded**: <https://github.com/aaronmyatt/homebrew-tap>, via `gh repo
   create aaronmyatt/homebrew-tap --public` + `brew tap-new aaronmyatt/tap` (Homebrew's own official
   scaffold — `Formula/`, a README, and **real, standard CI for free**:
   `.github/workflows/tests.yml` (`brew test-bot` on every formula PR), `autobump.yml` (daily `brew
   bump --open-pr` against upstream releases), `dependabot.yml`). The scaffold's generated
   `Formula/txtodo.rb` was replaced with this commit's `deploy/homebrew/Formula/txtodo.rb`
   verbatim (`brew style`/`brew audit --strict` both pass clean, verified against a local test tap
   before pushing — see `tasks/brew-distribution/notes.md`'s "As built" section), and
   `deploy/homebrew/update-formula.sh` was copied in as `bin/update-formula.sh`, documented in the
   tap's own README as a fallback tool, not the primary mechanism (see "Version bumps" below).
2. **`url`/`sha256` in the pushed formula are still placeholders** — pending the main repo's first
   real tagged release (`RELEASE_CI.patch.md`, itself still pending a human applying it and pushing
   a tag). Nothing to fetch or bump yet.

## Version bumps: two mechanisms, deliberately not choosing between them yet

- **Primary: `autobump.yml`** (already pushed, running on its own daily cron). `brew bump
  --open-pr` auto-detects a new upstream GitHub release via `livecheck` and opens a PR updating
  `url`/`sha256` — zero custom code, the standard mechanism every other Homebrew tap uses. Not yet
  proven against *this* formula's shape: two `resource` blocks (`txtodod`, `txtodo-tui`) inside
  `on_arm`/`on_intel`, not the common single-`url` case `brew bump` is best-tested against. First
  real proof needs a real upstream release to bump against.
- **Fallback: `bin/update-formula.sh <tag> <assets-dir>`** (pushed alongside it). Network-free,
  deterministic, already validated for real against this repo's own locally-built binaries (see
  `tasks/brew-distribution/notes.md`). If `autobump.yml`'s first real run mishandles the
  multi-resource shape, wire this script into a small custom workflow instead — triggered by a
  `repository_dispatch` `aaronmyatt/txtodo`'s own `release.yml` sends on every successful release,
  or the same daily-cron shape `autobump.yml` already uses:
  ```yaml
  - run: gh release download "$TAG" --repo aaronmyatt/txtodo --pattern '*macos*' --dir assets
  - run: ./bin/update-formula.sh "$TAG" assets
  - run: git commit -am "txtodo $TAG" && git push
  ```

## Still open, needs a human

- **A real `aaronmyatt/txtodo` release** (`RELEASE_CI.patch.md` applied + a tag pushed) — nothing
  above can be proven end-to-end without one. `brew install aaronmyatt/tap/txtodo
  --build-from-source`, `brew audit aaronmyatt/tap/txtodo`, `brew test aaronmyatt/tap/txtodo`, and
  whichever bump mechanism is chosen, all wait on this.
- **Cosign verification at bump time, not install time**: whichever bump mechanism ends up used
  should `cosign verify-blob` each asset's `.bundle` (`RELEASE_CI.patch.md`'s `sign` job produces
  one per binary) *before* pinning its sha256 into the formula, so a compromised release asset can
  never get a hash committed to the tap at all. `autobump.yml`'s stock `brew bump` doesn't do this;
  wiring it in (a custom step, or falling back to `update-formula.sh` plus an explicit verify step)
  is real, separate work once there's a real release to verify against.
