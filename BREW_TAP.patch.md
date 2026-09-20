# BREW_TAP.patch.md — status and remaining steps

Backlog item `id:01M2Q1BREWDISTRIBUTION01` (`brew-distribution`, plan M10) and its cask sibling
(`desktop-cask-distribution`). Unlike this repo's other `*.patch.md` handoffs, nothing here is
staged any more: the tap is a real repo, `aaronmyatt/homebrew-tap`
(<https://github.com/aaronmyatt/homebrew-tap>, `brew tap aaronmyatt/tap`), and everything below is
pushed. Last updated 2026-09-20, after v0.0.3.

## Correction to the root `todo.txt` line's own assumption

That line says the tap repo is `txtodo/homebrew-tap`. There is no `txtodo` GitHub org — the
project's real identity is the user `aaronmyatt`. Everything uses `aaronmyatt/homebrew-tap`,
Homebrew's own single-tap-per-user convention
(<https://docs.brew.sh/How-to-Create-and-Maintain-a-Tap>).

## What is live

- **`Formula/txtodo.rb`**: `txtodo`, `txtodod` and `txtodo-tui`, stamped against v0.0.2 on `main`
  (v0.0.3 is in PR #2, below). macOS gets the native builds (`on_macos`), Linux the fully static musl
  builds (`on_linux`), each split into `on_arm`/`on_intel` with `txtodod`/`txtodo-tui` as `resource`
  blocks: 12 url/sha256 pairs. The `on_linux` blocks exist because Homebrew's `readall` (run by
  `brew test-bot` on every OS/arch) refuses a formula that has no url on Linux. **No install has
  been tried on a real Linux machine**; only "the formula loads there" is proven.
- **`Casks/txtodo-desktop.rb`**: the Tauri app, `on_arm`/`on_intel`, `depends_on formula: "txtodo"`.
  The `app` stanza follows `productName` in `tauri.conf.json`: `desktop.app` through v0.0.2,
  `txtodo.app` from v0.0.3 (checked by mounting both `.dmg`s).
- **Version bumps: `.github/workflows/autobump.yml`** (daily 04:25 UTC, on demand, and on a change to
  the file). It replaced the `brew tap-new` scaffold's `brew bump --open-pr`, which cannot rewrite
  this formula: on a Linux runner it could not even load it ("formula requires at least a URL"),
  and on a macOS runner it died with `Could not find 'url' stanza!` (its rewriter only knows a
  top-level `url`, and it warns that this formula's resources "may need to be updated"). The
  workflow instead: finds the latest release, skips if the tap is on it or `bump/<tag>` exists,
  downloads the macOS and musl assets and `.dmg`s, verifies every Sigstore `.bundle` against that
  tag's own `release.yml` identity, stamps with `bin/update-formula.sh` and `bin/update-cask.sh`,
  checks all 12 formula and 2 cask url/sha256 pairs moved, checks the cask's `app` is inside each
  `.dmg`, then opens a PR from `bump/<tag>`.
- **Scripts**: the source of truth is `deploy/homebrew/` here (`update-formula.sh`,
  `update-cask.sh`, `stamp-cask-sha.py`, and `update-release.sh` to run both). The tap's `bin/`
  copies are identical except for the path to `Formula/`/`Casks/`; change both. The cask's sha
  stamper is a separate `.py` because `brew style`'s shell formatter deletes any heredoc body that
  contains a line starting with `if ` (`brew style --fix` truncates the script).
- **Tap CI**: `brew test-bot` is green on `main` on both macOS and Ubuntu.

## What the first real bump run showed (2026-09-20)

v0.0.3 was cut to give the autobump something to fire against. The answer to
`01M2Q1BREWAUTOBUMPCHECK01`'s question is **no: `brew bump` does not bump all 3 resources for this
formula shape** (see above), so it was replaced, not tuned. The replacement's first run opened
`aaronmyatt/homebrew-tap` PR #2, "txtodo v0.0.3". Two things only surfaced by reading that PR: it
predated the `on_linux` blocks (its Linux urls were still v0.0.2, now stamped), and the cask still
said `app "desktop.app"` while the v0.0.3 `.dmg` holds `txtodo.app` (fixed on the branch; the
workflow now fails a bump on that mismatch).

## Still open, needs a human

- **Review and merge tap PR #2**, then `brew update && brew upgrade txtodo && txtodo --version &&
  txtodod --version && txtodo-tui --version` (expect 0.0.3) and a `brew install --cask
  aaronmyatt/tap/txtodo-desktop`. A PR opened with the default `GITHUB_TOKEN` does not start
  `tests.yml`, so it has no CI; that is why this check is manual.
- **Try the Linux install** on a real Linux machine (`brew install aaronmyatt/tap/txtodo`).
- **Apple Gatekeeper**: no code-signing identity, no notarization. A fresh Mac flags the app
  "unidentified developer" until a human provisions an Apple Developer ID cert and `notarytool`
  credentials (`tasks/desktop-cask-distribution/notes.md`'s open question: ship unsigned first, or
  block the cask on the cert).
- **Tauri's minimum macOS version**: unconfirmed, so the cask's `depends_on :macos` has no floor.
- **Repo setting the workflow depends on**: Settings > Actions > General > "Allow GitHub Actions to
  create and approve pull requests" (turned on 2026-09-20).
- **Dependabot PR #1** on the tap (bumps three pinned actions) is unreviewed and predates these
  fixes, so its test-bot run failed before them; it needs a rebase to re-run.
