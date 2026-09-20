# brew-distribution

## Goal

`brew install txtodo` on macOS, fetching signed binaries from a real GitHub Release once
release-engineering (root todo.txt, `+m10 @release`) ships one.

## Correction to the root line's own assumption

The root line says the tap repo is `txtodo/homebrew-tap`. The real GitHub identity for this
project is the user `aaronmyatt` (`gh repo view` confirms `aaronmyatt/txtodo`), not an org called
`txtodo` — there is no `txtodo` org. Homebrew's own tap convention
(<https://docs.brew.sh/How-to-Create-and-Maintain-a-Tap>) names a tap repo `homebrew-<name>`,
addressed as `brew tap <user>/<name>`. This task uses `aaronmyatt/homebrew-tap` (`brew tap
aaronmyatt/tap`) throughout — the more common single-tap-per-user pattern — not
`aaronmyatt/homebrew-txtodo`, since this project ships three binaries from one crate family, not
one tool among many unrelated ones.

## Real, exact asset layout (read from `RELEASE_CI.patch.md`, not guessed)

Every artifact is a bare file, uploaded individually to the release (`gh release create ...
$(find artifacts -type f)` — no tarballs). Per macOS leg (`macos-aarch64`, `macos-x86_64`):
`txtodo-<leg>`, `txtodod-<leg>`, `txtodo-tui-<leg>`, each with a cosign `.bundle` sibling
(`<name>.bundle` — cosign 3.x's consolidated bundle format, not a separate `.sig`, correcting the
root line's "bom.json/sig" wording), plus one release-wide `bom.json`.

## What can be built and verified now vs. what needs a live release

Same discipline `RELEASE_CI.patch.md`/`RELAY_CONVERGE_CI.patch.md` already established for this
backlog: build and verify everything that doesn't require a release to exist; stage everything
that does, with the exact human handoff steps named.

- **Can verify now**: the formula's Ruby structure (`brew style`, `brew audit` minus the
  network-dependent checks), and the version-bump script's substitution logic against a real,
  locally-built macOS binary (this repo's own `cargo build --release -p txtodo-cli` — the same
  binary a real release would ship, just not fetched from a URL).
- **Cannot verify without a live release**: `brew install --build-from-source` end-to-end (needs a
  real, fetchable URL + a sha256 that matches what's actually posted), cosign bundle verification
  against a real release's real Sigstore transparency-log entry.

## As built (2026-09-17, agent)

`Formula/txtodo.rb` (`deploy/homebrew/Formula/txtodo.rb`): ships all three binaries per arch
(`on_macos`/`on_arm`/`on_intel`, `txtodod`/`txtodo-tui` as `resource` blocks), no explicit
`version` (Homebrew infers it from each `url`'s own `/v<tag>/` segment — `brew audit` flags an
explicit one matching the URL as redundant). Validated against a real local test tap
(`$(brew --repository)/Library/Taps/aaronmyatt/homebrew-tap`, removed after): `brew style` and
`brew audit --strict` both pass clean.

`deploy/homebrew/update-formula.sh`: given a tag and a directory of already-downloaded assets,
rewrites every `url`'s tag segment and each asset's own `sha256` (anchored so e.g.
`txtodo-macos-aarch64` can never false-match inside `txtodo-tui-macos-aarch64`). Network-free by
design. Run for real against this repo's own locally-built `txtodo`/`txtodod`/`txtodo-tui`
binaries, staged under fake `*-macos-{aarch64,x86_64}` names: all 6 url/sha256 pairs rewritten
correctly, right hash next to the right binary, zero cross-contamination. Fails cleanly (exit 1,
names the missing path) when an asset is absent.

**The tap repo itself is real, not staged**: the human asked for it to be created directly.
`gh repo create aaronmyatt/homebrew-tap --public`, then seeded via `brew tap-new aaronmyatt/tap`
— Homebrew's own official scaffold, which turned up something better than a hand-rolled workflow:
real `autobump.yml` (daily `brew bump --open-pr` against upstream GitHub releases, the standard
mechanism every tap uses) plus `tests.yml`/`dependabot.yml`, all for free. The scaffold's generated
formula was replaced with the real one above; `update-formula.sh` was copied in as
`bin/update-formula.sh`, documented as a fallback in case `autobump`'s per-resource handling of
this formula's two `resource` blocks doesn't hold up on its first real run — genuinely unproven
either way without a real upstream release to bump against. (It did not hold up, and the fallback
became the mechanism: see "Update (2026-09-20)" below.) Pushed to
<https://github.com/aaronmyatt/homebrew-tap> — see `BREW_TAP.patch.md` for the full status and
what's still open.

## RESOLVED (2026-09-17)

v0.0.1 published. Every macOS binary's cosign Sigstore signature verified against `release.yml`'s
own OIDC identity before stamping; `deploy/homebrew/Formula/txtodo.rb` pushed live to
`aaronmyatt/homebrew-tap`. `brew tap` + `brew audit --strict` + `brew install` + `brew test` all
run for real against the live tap and pass — installed `txtodo`/`txtodod`/`txtodo-tui` each report
`0.0.1`.

`autobump.yml`'s multi-resource handling is still unproven (needs a second real release to fire) —
that's the only remaining gap, tracked as its own follow-up
(`id:01M2Q1BREWAUTOBUMPCHECK01`, root todo.txt). (Answered 2026-09-20: it cannot do it. See below.)

## Cask sibling added (task desktop-cask-distribution, 2026-09-17)

A Homebrew Cask for `apps/desktop` was staged alongside this formula — `deploy/homebrew/
Casks/txtodo-desktop.rb`, `deploy/homebrew/update-cask.sh`, and a unified `update-release.sh`
wrapper that stamps both the formula and the cask from one release tag in one pass. See
`tasks/desktop-cask-distribution/notes.md` and `BREW_TAP.patch.md`'s own "Cask addition" section
for the full account — not duplicated here.

## Update (2026-09-20): `brew bump` replaced, formula gains Linux

v0.0.3 was cut to give the autobump a second real release to fire against. It could not do the job.

- **`brew bump` cannot bump this formula.** It had failed daily since 09-18 on its Ubuntu runner
  ("formula requires at least a URL": every `url` sat inside `on_macos`, so the formula would not
  load on Linux). On a macOS runner it got as far as downloading the v0.0.3 asset, then died with
  `Could not find 'url' stanza!` and a warning that the formula's resources "may need to be
  updated". Its rewriter only knows a top-level `url`; ours are nested in `on_macos`/`on_arm` and
  there are two `resource` blocks. So the answer to `01M2Q1BREWAUTOBUMPCHECK01` is no.
- **Replacement: the tap's `autobump.yml`.** Daily, on demand, and on a change to the file. It finds
  the latest release (skips if the tap is on it or `bump/<tag>` exists), downloads the macOS and
  static Linux assets and the `.dmg`s, verifies every Sigstore `.bundle` against that tag's own
  `release.yml` identity, stamps with `bin/update-formula.sh` and `bin/update-cask.sh`, checks all
  12 formula and 2 cask url/sha256 pairs moved, checks the cask's `app` is inside each `.dmg`, and
  opens a PR from `bump/<tag>`. It needs the repo setting "Allow GitHub Actions to create and
  approve pull requests" (a human turned it on). A PR opened with the default token does not start
  `tests.yml`; a later push to its branch by a person does, which is how PR #2 got its full
  install test.
- **The formula gained `on_linux`** (the static musl builds). `brew test-bot` runs `readall`, which
  loads every formula for every OS/arch, so a formula with no Linux url failed both test-bot legs.
  12 pairs now; test-bot is green on `main`. PR #2's Ubuntu leg fetched, installed, audited and ran
  `brew test` on x86_64 Linux; arm64 Linux and any machine of yours are untried.
- **Found only by reading the first PR (#2, "txtodo v0.0.3"):** it predated `on_linux` (Linux urls
  still v0.0.2; now stamped), and the cask still said `app "desktop.app"`. `productName` went
  from `desktop` to `txtodo` after v0.0.2: the v0.0.2 `.dmg` holds `desktop.app`, v0.0.3's holds
  `txtodo.app` (both mounted and checked). Fixed on the branch; the workflow now fails a bump on
  that mismatch.
- **`brew style` traps:** its shell formatter deletes any heredoc body containing a line that
  starts with `if ` (`brew style --fix` truncated `update-cask.sh`), so the Python moved to
  `stamp-cask-sha.py`. `brew style` also installs `shellcheck`, `shfmt` and `actionlint` on its own.
- **Scripts:** `deploy/homebrew/` here is the source of truth. The tap's `bin/` copies differ only
  in the path to `Formula/`/`Casks/`. Change both.
- **Result:** PR #2 was read and merged (`b30a3cc`, 2026-09-20). `brew install
  aaronmyatt/tap/txtodo` gave 0.0.3, and all three binaries report 0.0.3 by full path. A bare
  `txtodo --version` still said 0.0.2: `~/.local/bin` (dev symlinks into `target/release/`) is
  ahead of `/opt/homebrew/bin` on `PATH`. Root line `01M2Q1BREWAUTOBUMPCHECK01` is closed.
- **Still open, human:** `brew install --cask aaronmyatt/tap/txtodo-desktop` (move an existing
  `/Applications/txtodo.app` aside first; CI never installs the cask). The Linux arm64 install. The
  Gatekeeper and macOS-floor questions in `tasks/desktop-cask-distribution/notes.md`. Rebase the
  tap's dependabot PR #1. The first scheduled, unattended bump is still unseen.
