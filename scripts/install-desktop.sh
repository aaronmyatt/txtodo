#!/usr/bin/env bash
# Install the txtodo desktop app on macOS from the latest GitHub release, with no Gatekeeper block.
#
# Why this works unsigned: Gatekeeper only checks files carrying the com.apple.quarantine xattr
# (extended attribute), which browsers, Mail and AirDrop add on download. curl never adds it, so a
# .dmg fetched here — and the .app copied out of it — opens without the "unidentified developer"
# dialog. The .app is still ad-hoc signed by release.yml, which Apple Silicon needs to run it.
# Ref: https://developer.apple.com/documentation/security/gatekeeper
# Ref: https://developer.apple.com/documentation/bundleresources/information-property-list/lsfilequarantineenabled
#
# Usage:
#   curl -fsSL https://raw.githubusercontent.com/aaronmyatt/txtodo/main/scripts/install-desktop.sh | bash
#
# Env overrides:
#   TXTODO_VERSION=v0.0.8        install that tag instead of the latest release
#   TXTODO_INSTALL_DIR=<dir>     install somewhere other than /Applications (or ~/Applications)
set -euo pipefail

repo="aaronmyatt/txtodo"
app="txtodo.app"

if [ "$(uname -s)" != "Darwin" ]; then
  echo "install-desktop: macOS only (the desktop release ships as a .dmg)" >&2
  exit 1
fi

# release.yml names each dmg desktop-macos-<leg>.dmg; `uname -m` says arm64 on Apple Silicon.
# Ref: https://keith.github.io/xcode-man-pages/uname.1.html
case "$(uname -m)" in
  arm64) leg="macos-aarch64" ;;
  x86_64) leg="macos-x86_64" ;;
  *) echo "install-desktop: unsupported arch $(uname -m)" >&2; exit 1 ;;
esac
asset="desktop-${leg}.dmg"

# GitHub serves /releases/latest/download/<asset> as a redirect to the newest non-prerelease.
# Ref: https://docs.github.com/en/repositories/releasing-projects-on-github/linking-to-releases
if [ -n "${TXTODO_VERSION:-}" ]; then
  url="https://github.com/${repo}/releases/download/${TXTODO_VERSION}/${asset}"
else
  url="https://github.com/${repo}/releases/latest/download/${asset}"
fi

# /Applications when writable (admin users), else the per-user ~/Applications, so no sudo needed.
dest="${TXTODO_INSTALL_DIR:-}"
if [ -z "$dest" ]; then
  if [ -w /Applications ]; then dest="/Applications"; else dest="$HOME/Applications"; fi
fi
mkdir -p "$dest"

tmp="$(mktemp -d)"
mnt="$tmp/mnt"
# Detach the dmg and drop the temp dir however the script exits.
# Ref: https://www.gnu.org/software/bash/manual/html_node/Bourne-Shell-Builtins.html#index-trap
cleanup() {
  if [ -d "$mnt" ]; then hdiutil detach -quiet "$mnt" 2>/dev/null || true; fi
  rm -rf "$tmp"
}
trap cleanup EXIT

echo "install-desktop: downloading $url"
curl -fL --progress-bar -o "$tmp/$asset" "$url"

# -nobrowse keeps the volume out of Finder; -readonly since we only copy out of it.
# Ref: https://keith.github.io/xcode-man-pages/hdiutil.1.html
mkdir -p "$mnt"
hdiutil attach -quiet -nobrowse -readonly -mountpoint "$mnt" "$tmp/$asset"

if [ ! -d "$mnt/$app" ]; then
  echo "install-desktop: $app not found inside $asset" >&2
  exit 1
fi

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

echo "install-desktop: installed $dest/$app"
echo "install-desktop: open it with: open \"$dest/$app\""
