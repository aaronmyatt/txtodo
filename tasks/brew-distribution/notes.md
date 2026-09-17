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
either way without a real upstream release to bump against. Pushed to
<https://github.com/aaronmyatt/homebrew-tap> — see `BREW_TAP.patch.md` for the full status and
what's still open.
