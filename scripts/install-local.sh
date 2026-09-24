#!/usr/bin/env bash
# `just install`: a full install of whatever this checkout holds (dirty or not), for testing a
# pre-release build, e.g. on two devices at once. Tagged releases go through scripts/install.sh
# or brew instead; this never downloads anything.
#
# What it does, in order:
#   1. builds txtodo, txtodod, txtodo-mcp and txtodo-tui (release profile)
#   2. macOS: self-signs them, same cert and identifiers as release.yml (scripts/macos-selfsign.sh),
#      so the Keychain's "Always Allow" carries over between this and a tagged release
#   3. macOS: builds the desktop app around that same signed txtodod, and signs it
#   4. purges every other install, so what runs is this build: brew's txtodo formula and
#      txtodo-desktop cask, scripts/install.sh's ~/.local/bin copies and ~/Applications app, and
#      any other txtodo* binary or symlink on $PATH. Runs only after every build succeeded.
#   5. puts the app in /Applications and the binaries in $CARGO_HOME/bin (default ~/.cargo/bin;
#      `just repoint-service` uses it)
#   6. restarts the OS service on the new txtodod: a same-version daemon never auto-upgrades
#      (crates/txtodo-daemon-launch/src/upgrade.rs), so a pre-release build needs this restart
#   7. writes the build id (git describe) to <data dir>/txtodo/installed-build, to compare devices
#
# Env: TXTODO_NO_DESKTOP=1 skips the desktop app. TXTODO_ALLOW_ADHOC=1 installs unsigned when the
# "txtodo Self-Signed" cert is missing (every rebuild then re-prompts for the keychain password).
set -euo pipefail
cd "$(dirname "${BASH_SOURCE[0]}")/.."

log() { echo "install-local: $*"; }
die() { echo "install-local: $*" >&2; exit 1; }

os="$(uname -s)"
bins="txtodo txtodod txtodo-mcp txtodo-tui"
bin_dir="${CARGO_HOME:-$HOME/.cargo}/bin"
identity="${APPLE_SIGNING_IDENTITY:-txtodo Self-Signed}"
# e.g. v0.0.10-3-g1a2b3c4-dirty: the tag, commits since, commit, and uncommitted changes.
# Ref: https://git-scm.com/docs/git-describe
build_id="$(git describe --tags --always --dirty)"
signed=0

# --- preflight: fail before a 10-minute build, not after it --------------------------------------
for tool in cargo git; do command -v "$tool" >/dev/null || die "$tool not found"; done
if [ "$os" = "Darwin" ]; then
  [ -n "${TXTODO_NO_DESKTOP:-}" ] || command -v npm >/dev/null || die "npm not found (desktop build)"
  # No -v: the cert only shows as "valid" once trusted; codesign signs with it either way.
  # Ref: https://keith.github.io/xcode-man-pages/security.1.html
  if security find-identity -p codesigning | grep -qF "\"$identity\""; then
    signed=1
  elif [ -z "${TXTODO_ALLOW_ADHOC:-}" ]; then
    die "no \"$identity\" code-signing cert in your keychain. Import txtodo-signing.p12 (Keychain
  Access > File > Import Items, login keychain), or rerun with TXTODO_ALLOW_ADHOC=1"
  fi
fi
log "building $build_id"

# --- 1. build ------------------------------------------------------------------------------------
# `cargo metadata` gives the real target dir, worktree or not.
# Ref: https://doc.rust-lang.org/cargo/commands/cargo-metadata.html
target="$(cargo metadata --format-version 1 --no-deps | sed -E 's/.*"target_directory":"([^"]*)".*/\1/')"
cargo build --release --locked -p txtodo-cli -p txtodo-daemon -p txtodo-mcp -p txtodo-tui

# Sign and install copies, never target/release itself: other things may point into it.
stage="$(mktemp -d)"
trap 'rm -rf "$stage"' EXIT
for bin in $bins; do cp "$target/release/$bin" "$stage/$bin"; done

# --- 2. sign -------------------------------------------------------------------------------------
if [ "$signed" = 1 ]; then
  staged=()
  for bin in $bins; do staged+=("$stage/$bin"); done
  APPLE_SIGNING_IDENTITY="$identity" scripts/macos-selfsign.sh "${staged[@]}"
fi

# --- 3. desktop app build (macOS) -------------------------------------------------------------------
if [ "$os" = "Darwin" ] && [ -z "${TXTODO_NO_DESKTOP:-}" ]; then
  # Tauri bundles binaries/txtodod-<host triple> as the app's sidecar.
  # Ref: https://v2.tauri.app/develop/sidecar/#platform-specific-binaries
  mkdir -p apps/desktop/src-tauri/binaries
  cp "$stage/txtodod" "apps/desktop/src-tauri/binaries/txtodod-$(rustc --print host-tuple)"
  [ -d apps/desktop/node_modules ] || (cd apps/desktop && npm ci)
  # The APPLE_* vars are unset so Tauri doesn't try its own (Apple-certs-only) signing.
  # Ref: https://v2.tauri.app/reference/cli/#build
  (cd apps/desktop && env -u APPLE_CERTIFICATE -u APPLE_SIGNING_IDENTITY npm run tauri build -- --bundles app)
  app="$target/release/bundle/macos/txtodo.app"
  [ -d "$app" ] || die "no bundle at $app"
  if [ "$signed" = 1 ]; then APPLE_SIGNING_IDENTITY="$identity" scripts/macos-selfsign.sh "$app"; fi
fi

# --- 4. purge other installs, only now the build has succeeded -----------------------------------
# brew first, so it removes its own files and `brew upgrade` can't bring them back. The cask also
# deletes /Applications/txtodo.app, so the new app is copied in only after this.
# Ref: https://docs.brew.sh/Manpage#uninstall-remove-rm-options-installed_formulainstalled_cask-
# The cask goes before the formula: it depends on it, and brew refuses to remove a dependency.
if command -v brew >/dev/null; then
  if brew list --cask txtodo-desktop >/dev/null 2>&1; then
    log "purging brew cask txtodo-desktop"; brew uninstall --cask txtodo-desktop
  fi
  if brew list --formula txtodo >/dev/null 2>&1; then
    log "purging brew formula txtodo"; brew uninstall --formula txtodo
  fi
fi
# scripts/install.sh's default folder, on PATH or not, then anything else on PATH by these names.
purge=()
for bin in $bins; do
  purge+=("$HOME/.local/bin/$bin")
  while IFS= read -r found; do purge+=("$found"); done < <(which -a "$bin" 2>/dev/null || true)
done
for path in "${purge[@]}"; do
  case "$path" in "$bin_dir"/*) continue ;; esac
  # -L too: a dangling symlink (into a cleaned target/) fails -e but still shadows nothing useful.
  if [ -e "$path" ] || [ -L "$path" ]; then
    if rm -f "$path" 2>/dev/null; then log "purged $path"; else log "WARNING: can't remove $path (try: sudo rm $path)"; fi
  fi
done
if [ -d "$HOME/Applications/txtodo.app" ]; then
  rm -rf "$HOME/Applications/txtodo.app"; log "purged $HOME/Applications/txtodo.app"
fi

# --- 5. install ----------------------------------------------------------------------------------
if [ -n "${app:-}" ]; then
  # ditto keeps the code signature and xattrs. Ref: https://keith.github.io/xcode-man-pages/ditto.1.html
  osascript -e 'if application id "com.txtodo.desktop" is running then tell application id "com.txtodo.desktop" to quit'
  rm -rf /Applications/txtodo.app
  ditto "$app" /Applications/txtodo.app
  rm -rf "$app"
  log "installed /Applications/txtodo.app"
fi
# Copy beside the target, then rename over it: a rename is atomic and leaves a running txtodod on
# its old file, where writing into a running signed binary gets it killed.
# Ref: https://developer.apple.com/documentation/security/updating-mac-software
mkdir -p "$bin_dir"
for bin in $bins; do
  cp "$stage/$bin" "$bin_dir/.$bin.new"
  mv -f "$bin_dir/.$bin.new" "$bin_dir/$bin"
done
log "installed $bins into $bin_dir"

# --- 6. restart the service on the new daemon ----------------------------------------------------
# Same steps as `just repoint-service`. `env -u`: .cargo/config.toml sets TXTODO_NO_SERVICE=1,
# which would make `daemon install` refuse on purpose.
ctl() { env -u TXTODO_NO_SERVICE "$bin_dir/txtodo" daemon "$@"; }
ctl stop || true
ctl install --force
ctl start
# `start` returns before the socket answers: boot loads every workspace, and a first run of a newly
# signed txtodod waits on the Keychain prompt. Wait up to 2 minutes rather than fail the install.
for _ in $(seq 1 120); do ctl status >/dev/null 2>&1 && break; sleep 1; done
ctl status || log "WARNING: daemon not answering yet; check for a Keychain prompt, then: txtodo doctor"

# --- 7. build stamp + checks ---------------------------------------------------------------------
# Same data dir the daemon's socket uses: $XDG_DATA_HOME/txtodo, else ~/.local/share/txtodo.
data_dir="${XDG_DATA_HOME:-$HOME/.local/share}/txtodo"
mkdir -p "$data_dir"
echo "$build_id" > "$data_dir/installed-build"
log "build $build_id ($("$bin_dir/txtodo" --version)); compare devices with: cat $data_dir/installed-build"

found="$(command -v txtodo || true)"
if [ -z "$found" ]; then
  log "WARNING: txtodo is not on your PATH; add $bin_dir to PATH"
elif [ "$found" != "$bin_dir/txtodo" ]; then
  log "WARNING: 'txtodo' on your PATH is $found, not this install. All copies:"
  # `|| true` stops pipefail + set -e from aborting the script on a warning-only path.
  # Ref: https://www.gnu.org/software/bash/manual/html_node/The-Set-Builtin.html
  which -a txtodo | sed 's/^/  /' || true
  log "remove the others (brew uninstall txtodo, or delete stale symlinks) or put $bin_dir first"
fi
