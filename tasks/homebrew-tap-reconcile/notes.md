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

## As built

- Nothing yet. The 0.0.13 tap bump (tap commit `0bde8a7`) was done by hand and did not touch this
  repo's staged copies, deliberately — reconciling them is this task, not a silent side effect.
