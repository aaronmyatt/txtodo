# txtodo-desktop.rb — staged for the aaronmyatt/homebrew-tap repo (task desktop-cask-distribution;
# see tasks/desktop-cask-distribution/notes.md). Sibling of Formula/txtodo.rb: that formula installs
# the CLI binaries (`txtodo`/`txtodod`/`txtodo-tui`) into `bin/`; this cask installs the Tauri
# desktop app (`apps/desktop`) into /Applications — a GUI `.app` belongs in Casks/, not Formula/,
# per Homebrew's own split (https://docs.brew.sh/Cask-Cookbook).
#
# `url`/`sha256` below are placeholders (`v0.0.0`/all-zero hashes) pending this repo's first real
# release to build `.github/workflows/release.yml`'s new `build-desktop` job — mirrors how
# Formula/txtodo.rb itself was staged with placeholders before v0.0.1 (see BREW_TAP.patch.md).
# `deploy/homebrew/update-formula.sh` stamps real values in once release assets exist; this cask
# needs the same treatment (tracked in this task's own todo.txt).
#
# No explicit macOS version floor: Tauri 2's actual minimum supported macOS version isn't
# documented anywhere this session could verify offline (v2.tauri.app's distribution docs need
# network access this task's sandbox didn't have) — `depends_on :macos` below is the bare,
# unversioned form (Homebrew's own `brew style --fix` suggested exactly this), not a guessed
# floor. A human should tighten it to a real minimum once confirmed, per
# <https://v2.tauri.app/distribute/macos/>.
#
# Cask DSL: https://docs.brew.sh/Cask-Cookbook · on_arm/on_intel: same Formula::Arch selectors
# Formula/txtodo.rb already uses (https://rubydoc.brew.sh/Formula.html).
cask "txtodo-desktop" do
  version "0.0.19"

  on_arm do
    sha256 "30ad23e49734b3f333879a89e6deec69502fea816ce3fe937e54edba012a9d1b"

    url "https://github.com/aaronmyatt/txtodo/releases/download/v#{version}/desktop-macos-aarch64.dmg"
  end
  on_intel do
    sha256 "aa83b56241ad0d80bf8be6c5073af94d75b8ab95802db22e54f0a9af19953ffc"

    url "https://github.com/aaronmyatt/txtodo/releases/download/v#{version}/desktop-macos-x86_64.dmg"
  end

  name "txtodo Desktop"
  desc "Todo.sh-compatible desktop app with encrypted multi-device sync"
  homepage "https://github.com/aaronmyatt/txtodo"

  # Defense in depth for task `desktop-daemon-sidecar-bundle`: the primary fix is bundling
  # `txtodod` into the app itself as a Tauri sidecar (see `apps/desktop/src-tauri/tauri.conf.json`'s
  # `bundle.externalBin` and `daemon/spawn.rs`'s sidecar-first resolution), but a Homebrew install
  # of this cask should not rely on that alone — depending on the sibling `txtodo` formula (same
  # tap: `Formula/txtodo.rb`, which ships `txtodod` too, see its own header comment) guarantees a
  # working `txtodod` on `$PATH` even if the sidecar resolution ever regresses. Same-tap formula
  # reference, not `"aaronmyatt/tap/txtodo"` (that longer form is for a *different* tap).
  # Ref: https://docs.brew.sh/Cask-Cookbook#depends_on
  depends_on formula: "txtodo"
  depends_on :macos

  # `productName` in apps/desktop/src-tauri/tauri.conf.json is what `tauri build` actually names
  # the bundle. It was "desktop" through v0.0.2 (that .dmg holds desktop.app) and is "txtodo" from
  # v0.0.3 on (checked by mounting both .dmgs), so this stanza changes with the version bump.
  app "txtodo.app"

  # Same three locations Apple's own sandboxing/App Support convention puts a document-free
  # utility app's state in, keyed by tauri.conf.json's `identifier` — this app writes no other
  # user data outside the workspace directories it's pointed at (those are never touched by zap,
  # deliberately: they're the human's own todo.txt files, not app state).
  zap trash: [
    "~/Library/Application Support/com.txtodo.desktop",
    "~/Library/Caches/com.txtodo.desktop",
    "~/Library/Preferences/com.txtodo.desktop.plist",
    "~/Library/Saved Application State/com.txtodo.desktop.savedState",
  ]
end
