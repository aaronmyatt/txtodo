# txtodo.rb — staged for the aaronmyatt/homebrew-tap repo (task brew-distribution; see
# tasks/brew-distribution/notes.md for why the tap is aaronmyatt/homebrew-tap, not the root
# backlog line's original txtodo/homebrew-tap guess — there is no txtodo GitHub org). Every `url`
# and `sha256` below are stamped for real by `deploy/homebrew/update-formula.sh` against a real
# release's real assets, currently v0.0.19 — all 16 binaries' cosign sigstore signatures were
# verified against release.yml's own OIDC identity
# (https://github.com/aaronmyatt/txtodo/.github/workflows/release.yml@refs/tags/v0.0.19, issuer
# https://token.actions.githubusercontent.com) before stamping, and every stamped hash was
# re-checked against its own asset afterwards (2026-09-29).
#
# Ships all four release binaries (RELEASE_CI.patch.md's macos-{aarch64,x86_64} and static
# linux-{aarch64,x86_64}-musl legs):
# `txtodo` (the CLI, todo.sh-compatible commands), `txtodod` (the sync daemon), `txtodo-tui`
# (the ratatui client) and `txtodo-mcp` (the MCP server `txtodo mcp` runs) — matching what a real
# release actually publishes, not just the CLI alone.
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
      url "https://github.com/aaronmyatt/txtodo/releases/download/v0.0.19/txtodo-macos-aarch64"
      sha256 "a1c2046f72d28ba2bb94d0c475bd7376479647701926b76afc251e2357549e5f"
      resource "txtodod" do
        url "https://github.com/aaronmyatt/txtodo/releases/download/v0.0.19/txtodod-macos-aarch64"
        sha256 "06062caddad60fd50a767fbe80c7bd1cd38206c42eb2184cce5460bcbb7ab3a2"
      end
      resource "txtodo-tui" do
        url "https://github.com/aaronmyatt/txtodo/releases/download/v0.0.19/txtodo-tui-macos-aarch64"
        sha256 "ef3f98b9340fc2ae78df893285dd87a134ceb2e3447f4c8163b453dd0cc8275b"
      end
      resource "txtodo-mcp" do
        url "https://github.com/aaronmyatt/txtodo/releases/download/v0.0.19/txtodo-mcp-macos-aarch64"
        sha256 "fecb2411192418cda9d24fc0038065254d7448148e9658d08d833aec212e7f0f"
      end
    end
    on_intel do
      url "https://github.com/aaronmyatt/txtodo/releases/download/v0.0.19/txtodo-macos-x86_64"
      sha256 "b0137cbece3dd45503dbad90b8d1aa2419bd925a637b29a5ad734f8f3faecdaa"
      resource "txtodod" do
        url "https://github.com/aaronmyatt/txtodo/releases/download/v0.0.19/txtodod-macos-x86_64"
        sha256 "adb6090d48979bb240a6a7ca1ed4ceaa852f6b0d7583414bfce304d70b72bf27"
      end
      resource "txtodo-tui" do
        url "https://github.com/aaronmyatt/txtodo/releases/download/v0.0.19/txtodo-tui-macos-x86_64"
        sha256 "7e0b777492fcb477b723e1c590c64d9d95bce952031f73e3261c0b5017950f15"
      end
      resource "txtodo-mcp" do
        url "https://github.com/aaronmyatt/txtodo/releases/download/v0.0.19/txtodo-mcp-macos-x86_64"
        sha256 "6255367cff09c931945aab0e1f9480e670247f23bcd38151732cd1477dfee4a8"
      end
    end
  end

  # Linux ships the fully static musl builds: no libc dependency, so one binary works on any
  # distro Homebrew supports. The glibc build (txtodo-linux-x86_64-gnu) is deliberately not used.
  on_linux do
    on_arm do
      url "https://github.com/aaronmyatt/txtodo/releases/download/v0.0.19/txtodo-linux-aarch64-musl"
      sha256 "b06c28a68bba68491adb058017df91652efb39ffbd93f52205b31c07f725bcbd"
      resource "txtodod" do
        url "https://github.com/aaronmyatt/txtodo/releases/download/v0.0.19/txtodod-linux-aarch64-musl"
        sha256 "1e475ee36b52cb84d132457108263c7bdc75bd355ebc473abe6ed3b9564ce457"
      end
      resource "txtodo-tui" do
        url "https://github.com/aaronmyatt/txtodo/releases/download/v0.0.19/txtodo-tui-linux-aarch64-musl"
        sha256 "227148a092408f87a8c2155a3ac5aa6f8ca2b07fecfbb2c436eb63a2a35fc3cf"
      end
      resource "txtodo-mcp" do
        url "https://github.com/aaronmyatt/txtodo/releases/download/v0.0.19/txtodo-mcp-linux-aarch64-musl"
        sha256 "11bccc4f1d3e4eb4962b25f92ed6621d5b5ae71ffdc80f4a875cf88eaed33c2a"
      end
    end
    on_intel do
      url "https://github.com/aaronmyatt/txtodo/releases/download/v0.0.19/txtodo-linux-x86_64-musl"
      sha256 "300813ef3c7612970c9b2a14d3e03831209e8390e41c66a19e1e84547c2cf00c"
      resource "txtodod" do
        url "https://github.com/aaronmyatt/txtodo/releases/download/v0.0.19/txtodod-linux-x86_64-musl"
        sha256 "67dc4d35ae7220f359af81aefc2ab7717e8985be3d537ab4dd4ac3098f82a733"
      end
      resource "txtodo-tui" do
        url "https://github.com/aaronmyatt/txtodo/releases/download/v0.0.19/txtodo-tui-linux-x86_64-musl"
        sha256 "2dd3df475e25581a82f0ac30f79620adec3da6b94fb9c55461bb5426852193d6"
      end
      resource "txtodo-mcp" do
        url "https://github.com/aaronmyatt/txtodo/releases/download/v0.0.19/txtodo-mcp-linux-x86_64-musl"
        sha256 "e1a5fa5101acbff97fbe50aef8ca0476b1a1b4e0d46accce7b9cda60a1f374fd"
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

    # `txtodo mcp` execs the txtodo-mcp beside txtodo, so it goes in the same bin.
    resource("txtodo-mcp").stage do
      mcp = Dir["txtodo-mcp-*"].first
      chmod 0755, mcp
      bin.install mcp => "txtodo-mcp"
    end
  end

  test do
    assert_match version.to_s, shell_output("#{bin}/txtodo --version")
    assert_match version.to_s, shell_output("#{bin}/txtodod --version")
    assert_match version.to_s, shell_output("#{bin}/txtodo-mcp --version")
  end
end
