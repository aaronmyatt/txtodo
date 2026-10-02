# Homebrew tap / repo reconciliation

## Goal

The same three things exist in two repos and have forked in both directions. Found 2026-09-25
while bumping the tap to 0.0.13 by hand.

## Inventory (2026-09-25, before the 0.0.13 bump)

| Thing | this repo | `aaronmyatt/homebrew-tap` |
|---|---|---|
| `Formula/txtodo.rb` | v0.0.2, **16-asset** (ships `txtodo-mcp`) | v0.0.8, **12-asset** (no `txtodo-mcp`) |
| `Casks/txtodo-desktop.rb` | v0.0.2 | v0.0.8 |
| `update-formula.sh` | `deploy/homebrew/`, 16-asset | `bin/`, 12-asset (stale fork) |
| `update-cask.sh` | `deploy/homebrew/` | `bin/` |
| `stamp-cask-sha.py` | `deploy/homebrew/` | `bin/` |
| `update-release.sh` (one-pass wrapper) | `deploy/homebrew/` | **absent** |

So neither side is simply "ahead": the tap had newer *versions* stamped, this repo has the newer
*tooling*. The `.rb` files here were never updated after v0.0.2 because the real bumps were done
directly in the tap.

Two things that fell out of that fork:

- **The tap's formula never shipped `txtodo-mcp`**, which releases have carried since v0.0.11.
  `txtodo mcp` execs a `txtodo-mcp` beside `txtodo`, so on any brew install it had nothing to
  exec. Fixed in the tap as part of the 0.0.13 bump; this repo's staged copy was always right.
- **`update-release.sh` exists precisely to stop the formula and cask drifting from each other**
  (its own header cites the todo line "one release-bump script rewrites both together") — but it
  lives only here, while the bumps happen only in the tap, so it has never actually been the thing
  that ran.

`release.yml` contains no tap/brew step at all: bumping the tap is entirely manual today.

## How the 0.0.13 bump was done by hand (repeat this until automated)

```bash
gh release download v0.0.13 --pattern '*macos*' --pattern '*linux*-musl*' --dir assets/
deploy/homebrew/update-release.sh v0.0.13 assets/   # stamps formula + cask together
```

Verify before stamping, not after — `cosign verify-blob --bundle <asset>.bundle
--certificate-identity https://github.com/aaronmyatt/txtodo/.github/workflows/release.yml@refs/tags/<tag>
--certificate-oidc-issuer https://token.actions.githubusercontent.com <asset>`. Note the
`--pattern '*-musl'` form does **not** pull the `.bundle` siblings; ask for them explicitly.

Then re-check each stamped hash against its own asset. The formula orders `url` then `sha256`; the
cask orders `sha256` then `url` (`brew style`'s Cask/StanzaOrder). That inversion is exactly how a
hash ends up attached to the wrong URL, which is why both stamp scripts anchor per-asset — worth
re-verifying independently rather than trusting the sed.

## Design

Pick one canonical home, delete the other copy, then make the tap update happen at release time
instead of by hand.

`.github/**` is frozen (`.claude/budgets.json` `slices.frozenPaths`, no unlock sentinel), so any
`release.yml` change ships as a patch file for a human to paste — same constraint as
[[ci-runner-deprecations]].

## Open question (`@human`)

How does the tap get updated?

- **Manual, canonical here**: keep the scripts and `.rb` files in this repo, delete the tap's
  `bin/`, and copy the two stamped `.rb`s over after each release. No new secret. Still a manual
  step someone forgets — which is how we got here.
- **Automated from `release.yml`**: a post-publish job stamps and pushes to the tap. Needs a token
  with write access to a *different* repo (a PAT or a GitHub App installation token) held as a
  secret — that is a real credential decision, and it widens what a compromised release run can
  write to. Not an agent's call.

I'd go canonical-here plus automated, but the credential is the human's to choose and provision.

**Decided 2026-10-02 (human): canonical here, pushed by `release.yml`.** The tap's `bin/` copy goes.
The token (a fine-grained PAT with contents write on the tap only, or a GitHub App) is the
human's to create and store as a repo secret; the job is staged as a patch, since `.github` is frozen.

## As built

- Nothing yet. The 0.0.13 tap bump (tap commit `0bde8a7`) was done by hand and did not touch this
  repo's staged copies, deliberately — reconciling them is this task, not a silent side effect.
- 2026-10-02: this repo's `deploy/homebrew/Formula/txtodo.rb` and `Casks/txtodo-desktop.rb` copied
  verbatim from the tap at `befb1fa` (v0.0.19; the formula here was at v0.0.13, the cask at v0.0.2).
  Both pass `ruby -c`. The tap moved on by hand twice since this task was filed (0.0.14, 0.0.19);
  until the decide line lands, the next bump will fork them again.

## Found 2026-10-02: the tap already bumps itself
- The tap has `.github/workflows/autobump.yml` (since 2026-09-20, see `BREW_TAP.patch.md`): daily at
  04:25 UTC and on demand, it downloads the newest release, verifies every Sigstore bundle, stamps
  with the tap's `bin/update-formula.sh` and `bin/update-cask.sh`, and opens a PR from
  `bump/<tag>`. It runs green every day; the tap is on v0.0.19, the latest release. The inventory
  above (09-25) missed it, so "bumping the tap is manual" was already false when this was decided.
- Two PRs are still open in the tap: #9 (v0.0.15) and #8 (v0.0.10), both superseded.
- So a `release.yml` push job and a PAT buy only speed (minutes, not up to a day). The fork is the
  real problem, and it has a no-secret fix: autobump fetches `deploy/homebrew/*` from this repo
  at the release tag (public raw URL) instead of running its own `bin/` copy; then `bin/` goes.
- Options for the owner:
  - A: keep autobump, point it at this repo's scripts, delete the tap's `bin/`. No token.
  - B: as decided: `release.yml` job + PAT pushes to the tap; autobump stays as a fallback or goes.
  - I'd take A: same result a day later at worst, no cross-repo credential.

## Decided 2026-10-02 (owner): A. As built
- The tap's `autobump.yml` fetches `deploy/homebrew/{update-formula.sh,update-cask.sh,stamp-cask-sha.py}`
  from this repo at the release tag (contents API, raw) and runs them from a folder that links to
  the tap's `Formula/` and `Casks/`. The tap's `bin/` fork is gone, with a one-line pointer
  (`bin/README.md`). Tap commit b23370b.
- Its checks were still the 12-asset ones while the formula has 16 urls (txtodo-mcp), so the next
  auto bump would have failed; now 16. Dry run with fake assets at v0.0.20: 16 urls, 16 hashes,
  cask version and 2 hashes moved.
- Closed the tap's stale bump PRs #8 (v0.0.10) and #9 (v0.0.15).
- No `release.yml` job, no token. Known gap: the bump can lag a release by up to a day (daily
  cron), and the new stamp step has only run dry; the first real run is the 0.0.20 bump.
