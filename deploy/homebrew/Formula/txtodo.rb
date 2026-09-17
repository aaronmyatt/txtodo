# txtodo.rb — staged for the aaronmyatt/homebrew-tap repo (task brew-distribution; see
# tasks/brew-distribution/notes.md for why the tap is aaronmyatt/homebrew-tap, not the root
# backlog line's original txtodo/homebrew-tap guess — there is no txtodo GitHub org). Every `url`
# and `sha256` below are stamped for real by `deploy/homebrew/update-formula.sh` against a real
# release's real assets, currently v0.0.2 — each binary's cosign sigstore signature was verified
# against release.yml's own OIDC identity before stamping (2026-09-18).
#
# Ships all three release binaries (RELEASE_CI.patch.md's macos-aarch64/macos-x86_64 legs):
# `txtodo` (the CLI, todo.sh-compatible commands), `txtodod` (the sync daemon) and `txtodo-tui`
# (the ratatui client) — matching what a real release actually publishes, not just the CLI alone.
#
# Homebrew formula cookbook: https://docs.brew.sh/Formula-Cookbook
# on_macos/on_arm/on_intel DSL: https://rubydoc.brew.sh/Formula.html
class Txtodo < Formula
  desc "Todo.sh-compatible CLI with real-time, end-to-end-encrypted multi-device sync"
  homepage "https://github.com/aaronmyatt/txtodo"
  license any_of: ["MIT", "Apache-2.0"]
  # No explicit `version`: Homebrew infers it from each `url`'s own `/v<version>/` path segment
  # (`brew audit` flags an explicit one matching the URL as redundant) — update-formula.sh only
  # ever needs to rewrite urls/sha256s, never a separate version field.

  on_macos do
    on_arm do
      url "https://github.com/aaronmyatt/txtodo/releases/download/v0.0.2/txtodo-macos-aarch64"
      sha256 "16155b110ea1be0b1984742f41db8c8751a724614253e5a6c691215bb06d1843"
      resource "txtodod" do
        url "https://github.com/aaronmyatt/txtodo/releases/download/v0.0.2/txtodod-macos-aarch64"
        sha256 "75bd25ac4fadea6be9473d84b00d101ca6f4b2148f0f95898a4053dcaa259a78"
      end
      resource "txtodo-tui" do
        url "https://github.com/aaronmyatt/txtodo/releases/download/v0.0.2/txtodo-tui-macos-aarch64"
        sha256 "2d35ee31558f68a5443ddc94b7d4e369fd3cf750da7e8fdcb29ba955c4619482"
      end
    end
    on_intel do
      url "https://github.com/aaronmyatt/txtodo/releases/download/v0.0.2/txtodo-macos-x86_64"
      sha256 "f711f0f5a3b4e71d6366775ee94beff163e67d2e9e8bb92d44b0e94e4bc575b7"
      resource "txtodod" do
        url "https://github.com/aaronmyatt/txtodo/releases/download/v0.0.2/txtodod-macos-x86_64"
        sha256 "4c4d54459ab2ddca6252fd556e69f2edd3a846574150aadea212fd17123c060e"
      end
      resource "txtodo-tui" do
        url "https://github.com/aaronmyatt/txtodo/releases/download/v0.0.2/txtodo-tui-macos-x86_64"
        sha256 "54dd0afbe668a956328f389765778075aedd90e6ace6126ad772bde80a8eee8c"
      end
    end
  end

  # Every asset is a bare downloaded binary, not an archive — Homebrew stages the primary `url`
  # download under its own URL-derived basename (e.g. "txtodo-macos-aarch64"), and each `resource`
  # under `resource_name` (its own default staging name); GitHub Release assets carry no unix
  # executable bit over HTTP, so every binary needs an explicit `chmod` before `bin.install`.
  def install
    cli = Dir["txtodo-*"].first
    chmod 0755, cli
    bin.install cli => "txtodo"

    resource("txtodod").stage do
      daemon = Dir["txtodod-*"].first
      chmod 0755, daemon
      bin.install daemon => "txtodod"
    end

    resource("txtodo-tui").stage do
      tui = Dir["txtodo-tui-*"].first
      chmod 0755, tui
      bin.install tui => "txtodo-tui"
    end
  end

  test do
    assert_match version.to_s, shell_output("#{bin}/txtodo --version")
    assert_match version.to_s, shell_output("#{bin}/txtodod --version")
  end
end
