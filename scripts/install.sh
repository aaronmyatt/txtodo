#!/usr/bin/env bash
# Install txtodo on macOS from the latest GitHub release, with no Gatekeeper block: the desktop
# app into /Applications, and the txtodo, txtodod, txtodo-mcp and txtodo-tui binaries into
# ~/.local/bin.
#
# Why this works unnotarized: Gatekeeper only checks files carrying the com.apple.quarantine xattr
# (extended attribute), which browsers, Mail and AirDrop add on download. curl never adds it, so
# what this script fetches runs without the "unidentified developer" dialog. release.yml signs
# everything with the self-signed "txtodo Self-Signed" cert (scripts/macos-selfsign.sh).
# Ref: https://developer.apple.com/documentation/security/gatekeeper
# Ref: https://developer.apple.com/documentation/bundleresources/information-property-list/lsfilequarantineenabled
#
# Usage:
#   curl -fsSL https://raw.githubusercontent.com/aaronmyatt/txtodo/main/scripts/install.sh | bash
#
# Env overrides:
#   TXTODO_VERSION=v0.0.9        install that tag instead of the latest release
#   TXTODO_INSTALL_DIR=<dir>     app folder (default /Applications, or ~/Applications if not writable)
#   TXTODO_BIN_DIR=<dir>         binaries folder (default ~/.local/bin)
#   TXTODO_NO_DESKTOP=1          skip the desktop app
#   TXTODO_NO_CLI=1              skip txtodo, txtodod, txtodo-mcp and txtodo-tui
set -euo pipefail

repo="aaronmyatt/txtodo"
app="txtodo.app"
# txtodo finds the txtodod beside it first (crates/txtodo-daemon-launch/src/binary_path.rs), so
# they all go into one folder.
bins="txtodo txtodod txtodo-mcp txtodo-tui"

log() { echo "install: $*"; }
die() { echo "install: $*" >&2; exit 1; }

[ "$(uname -s)" = "Darwin" ] || die "macOS only"

# Release assets are named <name>-macos-<leg>; `uname -m` says arm64 on Apple Silicon.
# Ref: https://keith.github.io/xcode-man-pages/uname.1.html
case "$(uname -m)" in
  arm64) leg="macos-aarch64" ;;
  x86_64) leg="macos-x86_64" ;;
  *) die "unsupported arch $(uname -m)" ;;
esac

# GitHub serves /releases/latest/download/<asset> as a redirect to the newest non-prerelease.
# Ref: https://docs.github.com/en/repositories/releasing-projects-on-github/linking-to-releases
if [ -n "${TXTODO_VERSION:-}" ]; then
  base="https://github.com/${repo}/releases/download/${TXTODO_VERSION}"
else
  base="https://github.com/${repo}/releases/latest/download"
fi

tmp="$(mktemp -d)"
mnt="$tmp/mnt"
# Detach the dmg and drop the temp dir however the script exits.
# Ref: https://www.gnu.org/software/bash/manual/html_node/Bourne-Shell-Builtins.html#index-trap
cleanup() {
  if [ -d "$mnt" ]; then hdiutil detach -quiet "$mnt" 2>/dev/null || true; fi
  rm -rf "$tmp"
}
trap cleanup EXIT

install_desktop() {
  local asset="desktop-${leg}.dmg" dest="${TXTODO_INSTALL_DIR:-}"
  # /Applications when writable (admin users), else per-user ~/Applications: no sudo needed.
  if [ -z "$dest" ]; then
    if [ -w /Applications ]; then dest="/Applications"; else dest="$HOME/Applications"; fi
  fi
  mkdir -p "$dest"

  log "downloading $base/$asset"
  curl -fL --progress-bar -o "$tmp/$asset" "$base/$asset"

  # -nobrowse keeps the volume out of Finder; -readonly since we only copy out of it.
  # Ref: https://keith.github.io/xcode-man-pages/hdiutil.1.html
  mkdir -p "$mnt"
  hdiutil attach -quiet -nobrowse -readonly -mountpoint "$mnt" "$tmp/$asset"
  [ -d "$mnt/$app" ] || die "$app not found inside $asset"

  # Quit a running copy first so the replaced bundle isn't in use. Errors mean it wasn't running.
  # Ref: https://developer.apple.com/library/archive/documentation/AppleScript/Conceptual/AppleScriptLangGuide/reference/ASLR_cmds.html#//apple_ref/doc/uid/TP40000983-CH216-SW52
  osascript -e 'quit app "txtodo"' >/dev/null 2>&1 || true

  # Remove the old bundle before copying: ditto merges into an existing dir, which would leave
  # stale files from the previous version behind. ditto keeps the signature and xattrs intact.
  # Ref: https://keith.github.io/xcode-man-pages/ditto.1.html
  rm -rf "${dest:?}/$app"
  ditto "$mnt/$app" "$dest/$app"
  # Belt and braces: clear any quarantine flag in case the dmg came from somewhere that set one.
  # Ref: https://keith.github.io/xcode-man-pages/xattr.1.html
  xattr -dr com.apple.quarantine "$dest/$app" 2>/dev/null || true

  log "installed $dest/$app"
}

install_cli() {
  local dir="${TXTODO_BIN_DIR:-$HOME/.local/bin}" bin
  mkdir -p "$dir"
  for bin in $bins; do
    log "downloading $base/$bin-$leg"
    # Download beside the target, then rename over it. A rename is atomic and leaves a running
    # txtodod on its old file; writing into a running signed binary gets it killed by the kernel.
    # Ref: https://developer.apple.com/documentation/security/updating-mac-software
    if ! curl -fL --progress-bar -o "$dir/.$bin.download" "$base/$bin-$leg"; then
      rm -f "$dir/.$bin.download"
      # Releases up to v0.0.10 shipped no txtodo-mcp; `txtodo mcp` needs it beside txtodo.
      [ "$bin" = "txtodo-mcp" ] || die "download failed: $base/$bin-$leg"
      log "this release has no txtodo-mcp; skipped (\`txtodo mcp\` won't work)"
      continue
    fi
    chmod 755 "$dir/.$bin.download"
    mv -f "$dir/.$bin.download" "$dir/$bin"
  done
  log "installed $bins into $dir ($("$dir/txtodo" --version))"
  # A running older daemon restarts itself on the next txtodo call
  # (crates/txtodo-daemon-launch/src/upgrade.rs), so no restart here.

  case ":$PATH:" in
    *":$dir:"*) ;;
    *) log "$dir is not on your PATH; add this to ~/.zshrc: export PATH=\"$dir:\$PATH\"" ;;
  esac
}

[ -n "${TXTODO_NO_DESKTOP:-}" ] || install_desktop
[ -n "${TXTODO_NO_CLI:-}" ] || install_cli
