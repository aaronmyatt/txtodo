# txtodo.rb — staged for the aaronmyatt/homebrew-tap repo (task brew-distribution; see
# tasks/brew-distribution/notes.md for why the tap is aaronmyatt/homebrew-tap, not the root
# backlog line's original txtodo/homebrew-tap guess — there is no txtodo GitHub org). `version`,
# every `url` and every `sha256` below are placeholders, stamped for real by
# `deploy/homebrew/update-formula.sh` against a real release's real assets — see that script and
# BREW_TAP.patch.md for the human handoff this needs (no live release exists yet).
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
      url "https://github.com/aaronmyatt/txtodo/releases/download/v0.0.0/txtodo-macos-aarch64"
      sha256 "0000000000000000000000000000000000000000000000000000000000000000" # placeholder
      resource "txtodod" do
        url "https://github.com/aaronmyatt/txtodo/releases/download/v0.0.0/txtodod-macos-aarch64"
        sha256 "0000000000000000000000000000000000000000000000000000000000000000"
      end
      resource "txtodo-tui" do
        url "https://github.com/aaronmyatt/txtodo/releases/download/v0.0.0/txtodo-tui-macos-aarch64"
        sha256 "0000000000000000000000000000000000000000000000000000000000000000"
      end
    end
    on_intel do
      url "https://github.com/aaronmyatt/txtodo/releases/download/v0.0.0/txtodo-macos-x86_64"
      sha256 "0000000000000000000000000000000000000000000000000000000000000000"
      resource "txtodod" do
        url "https://github.com/aaronmyatt/txtodo/releases/download/v0.0.0/txtodod-macos-x86_64"
        sha256 "0000000000000000000000000000000000000000000000000000000000000000"
      end
      resource "txtodo-tui" do
        url "https://github.com/aaronmyatt/txtodo/releases/download/v0.0.0/txtodo-tui-macos-x86_64"
        sha256 "0000000000000000000000000000000000000000000000000000000000000000"
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
