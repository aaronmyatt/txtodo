# txtodo.rb — staged for the aaronmyatt/homebrew-tap repo (task brew-distribution; see
# tasks/brew-distribution/notes.md for why the tap is aaronmyatt/homebrew-tap, not the root
# backlog line's original txtodo/homebrew-tap guess — there is no txtodo GitHub org). Every `url`
# and `sha256` below are stamped for real by `deploy/homebrew/update-formula.sh` against a real
# release's real assets, currently v0.0.13 — all 16 binaries' cosign sigstore signatures were
# verified against release.yml's own OIDC identity
# (https://github.com/aaronmyatt/txtodo/.github/workflows/release.yml@refs/tags/v0.0.13, issuer
# https://token.actions.githubusercontent.com) before stamping, and every stamped hash was
# re-checked against its own asset afterwards (2026-09-25).
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
      url "https://github.com/aaronmyatt/txtodo/releases/download/v0.0.13/txtodo-macos-aarch64"
      sha256 "5a12d68883250e2d618a1eddf5da9f4da63f3f7e7f8f5cf1fdfdfadb603111fe"
      resource "txtodod" do
        url "https://github.com/aaronmyatt/txtodo/releases/download/v0.0.13/txtodod-macos-aarch64"
        sha256 "e7083aef58797c421203298fa9f8af9bc534a8519e03d0893b8ae46d42b9e8f2"
      end
      resource "txtodo-tui" do
        url "https://github.com/aaronmyatt/txtodo/releases/download/v0.0.13/txtodo-tui-macos-aarch64"
        sha256 "c6ec4b913779d3176d185acca38b56de19e375abbb004d484a2c0a73e92b8da3"
      end
      resource "txtodo-mcp" do
        url "https://github.com/aaronmyatt/txtodo/releases/download/v0.0.13/txtodo-mcp-macos-aarch64"
        sha256 "b2ac388de3fdc90148c1e46a3a740ee37db550120cf211b75503e1e99a9f8682"
      end
    end
    on_intel do
      url "https://github.com/aaronmyatt/txtodo/releases/download/v0.0.13/txtodo-macos-x86_64"
      sha256 "9f119d0096d948e4de6232d7765b9fa6d7bbbf2c9a9bcec02ec72e5a056b2b43"
      resource "txtodod" do
        url "https://github.com/aaronmyatt/txtodo/releases/download/v0.0.13/txtodod-macos-x86_64"
        sha256 "2a7180d69115761541aee7dce82215654fccdf1c083666a3cd7c9c8a3760799c"
      end
      resource "txtodo-tui" do
        url "https://github.com/aaronmyatt/txtodo/releases/download/v0.0.13/txtodo-tui-macos-x86_64"
        sha256 "d344e34811d40dec17632df3ed7b66f2f78b31be58af55673eaa5e15123e2c9a"
      end
      resource "txtodo-mcp" do
        url "https://github.com/aaronmyatt/txtodo/releases/download/v0.0.13/txtodo-mcp-macos-x86_64"
        sha256 "253acae3497db92a717e3e268a3a34c9965031a2b378efc2cbb7edef11b3dbcd"
      end
    end
  end

  # Linux ships the fully static musl builds: no libc dependency, so one binary works on any
  # distro Homebrew supports. The glibc build (txtodo-linux-x86_64-gnu) is deliberately not used.
  on_linux do
    on_arm do
      url "https://github.com/aaronmyatt/txtodo/releases/download/v0.0.13/txtodo-linux-aarch64-musl"
      sha256 "2f54719a46ad0bea0e099a5bd80c8425bf1fce37b53f1e9139fc72415f7c93ae"
      resource "txtodod" do
        url "https://github.com/aaronmyatt/txtodo/releases/download/v0.0.13/txtodod-linux-aarch64-musl"
        sha256 "692eee42e1a385fd3fcc2597ccbab23092feefa4d9ab7e7babc031e91deb684c"
      end
      resource "txtodo-tui" do
        url "https://github.com/aaronmyatt/txtodo/releases/download/v0.0.13/txtodo-tui-linux-aarch64-musl"
        sha256 "716ca1299bfeb28823d5207ac2a24e3bd9f6679025a575ff9f8568777673bb13"
      end
      resource "txtodo-mcp" do
        url "https://github.com/aaronmyatt/txtodo/releases/download/v0.0.13/txtodo-mcp-linux-aarch64-musl"
        sha256 "b728662ee328f7783da9b744a12defa8248fa05f6b24e33b86141b9ea38b56d2"
      end
    end
    on_intel do
      url "https://github.com/aaronmyatt/txtodo/releases/download/v0.0.13/txtodo-linux-x86_64-musl"
      sha256 "48b28d0435f33713e74653c6d652bb8b328981d55f45572e0592cf1eba0fe454"
      resource "txtodod" do
        url "https://github.com/aaronmyatt/txtodo/releases/download/v0.0.13/txtodod-linux-x86_64-musl"
        sha256 "5d5ad9e9ca0bc7108c38c897984f545a1de566d69d22b7995691b3f3d50f1198"
      end
      resource "txtodo-tui" do
        url "https://github.com/aaronmyatt/txtodo/releases/download/v0.0.13/txtodo-tui-linux-x86_64-musl"
        sha256 "9d7488abeb2f180a04755df13de031b329c398a4c49f7c073852d44f4c3871f1"
      end
      resource "txtodo-mcp" do
        url "https://github.com/aaronmyatt/txtodo/releases/download/v0.0.13/txtodo-mcp-linux-x86_64-musl"
        sha256 "ecd8312258c3cd441abf0e934088695cf68eaf0bda221feca66a98d47d26d96f"
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
