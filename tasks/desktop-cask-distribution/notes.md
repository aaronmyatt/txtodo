# desktop-cask-distribution

## Goal

`brew install --cask aaronmyatt/tap/txtodo-desktop` on macOS, installing the Tauri desktop app
(`apps/desktop`) from a signed GitHub Release asset, kept in lockstep with every release bump the
same way [brew-distribution](../brew-distribution/notes.md) does for `txtodo`/`txtodod`/`txtodo-tui`.

## Why a cask, not a formula

- A **formula** builds/installs a CLI binary into `bin/`. That's what `deploy/homebrew/Formula/txtodo.rb`
  already does for the 3 release binaries.
- A **cask** installs a GUI `.app` bundle into `/Applications` via Homebrew Cask's own DSL
  (`app`, `pkg`, `zap` stanzas). `apps/desktop` is a Tauri app producing a real `.app`/`.dmg`, so
  it belongs in `Casks/`, not `Formula/`. Casks and formulae are separate directories in the same
  tap repo (`aaronmyatt/homebrew-tap`) — no second tap needed.
- Ref: <https://docs.brew.sh/Cask-Cookbook>

## Current gaps (as of 2026-09-17)

- `.github/workflows/release.yml` has no build/upload step for `apps/desktop` at all — it only
  builds `txtodo-cli`/`txtodo-daemon`/`txtodo-tui` via cargo-zigbuild across its leg matrix
  (`linux-x86_64-gnu`, `linux-x86_64-musl`, `macos-aarch64`, `macos-x86_64`, `windows-x86_64-gnu`
  shipping tui-only per ADR 0010).
- `deploy/homebrew/Formula/txtodo.rb` ships only the 3 CLI-ish binaries (confirmed by reading it
  directly — `on_macos`/`on_arm`/`on_intel` + two `resource` blocks, no desktop app reference).
- `apps/desktop/src-tauri/tauri.conf.json` exists (`productName: "desktop"`, `identifier:
  com.txtodo.desktop`, `bundle.targets: "all"`) but nothing in CI ever runs `tauri build` against
  it — it's dev-only today (`desktop-stack-gaps`, root todo, already flagged this app has zero CI
  coverage at all, a separate still-open gap).

## Scope for this ref

1. **CI build legs**: add macOS `tauri build --bundles app,dmg` steps (aarch64 native, x86_64
   either native on an intel runner or via Tauri's own cross-build support — needs checking, Tauri
   cross-compiling macOS arches is less mature than cargo-zigbuild) producing a `.app`/`.dmg` per
   arch, uploaded as release assets the same way the 3 binaries are (`gh release create ... $(find
   artifacts -type f)`).
2. **Signing parity**: the existing release pipeline cosign-signs every binary (Sigstore keyless,
   OIDC identity) — extend that same `sign-blob` step to the desktop artifact(s) so verification
   is uniform across every shipped asset.
3. **Apple Gatekeeper — genuinely separate, human-gated concern**: cosign proves *provenance*
   (this CI built it), not Apple's own code-signing trust. A `.app` built with no Apple Developer
   ID signature + notarization will be blocked or scary-dialog'd by Gatekeeper on a fresh Mac
   regardless of cosign. That needs an Apple Developer Program membership, a Developer ID
   Application cert, and `xcrun notarytool` credentials — real secrets only a human can provision
   (not something to fake or skip silently). Tauri has first-class support for this once the certs
   exist (`tauri.conf.json`'s `bundle.macOS.signingIdentity` / notarization env vars,
   <https://v2.tauri.app/distribute/sign/macos/>) but provisioning the cert itself is out of an
   agent's reach.
4. **The cask file** (`deploy/homebrew/Casks/txtodo-desktop.rb`): `url`/`sha256` per release,
   `app "desktop.app"` (or whatever `productName` ends up building as), a `zap` stanza for clean
   uninstall (app support dirs, prefs plist under `com.txtodo.desktop`), `depends_on
   macos: ">= :sequoia"` or whatever floor Tauri 2 actually needs — check, don't guess.
5. **Bump automation**: `update-formula.sh` only touches `Formula/txtodo.rb` today. Either extend
   it to also rewrite `Casks/txtodo-desktop.rb`'s url/sha256/version, or add a sibling script —
   either way, one release tag should update both in one pass, not two manually-run scripts that
   can drift. Note: `aaronmyatt/homebrew-tap`'s own `autobump.yml` (from `brew tap-new`) may
   already handle a cask's version bump automatically the same way it (unproven-so-far, see
   `brew-distribution` root todo item `01M2Q1BREWAUTOBUMPCHECK01`) handles the formula's — check
   before building a redundant script.
6. **Offline-verifiable now**: `brew style`/`brew audit --strict` against the drafted cask, same
   discipline `brew-distribution` used for the formula — no live release needed for that part.
7. **Cannot verify without a live release + real signing**: `brew install --cask` end-to-end,
   Gatekeeper actually accepting the app on a clean machine.

## Open question for the human (item 3 above)

Do we ship the cask **unsigned/unnotarized first** (works, but every user sees a Gatekeeper
"unidentified developer" warning and must right-click-Open once) and revisit signing later once
an Apple Developer ID is available, or **block the whole cask** on getting that cert first? Not an
agent's call — flagged `@human` on the sub-task, not silently assumed either way.
