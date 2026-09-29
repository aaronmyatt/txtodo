#!/usr/bin/env bash
# Signs each path with the "txtodo Self-Signed" cert (scripts/macos-selfsign.sh) when this Mac has
# it, so every local build carries the same code requirement as `just install` and release.yml and
# the Keychain's "Always Allow" given once keeps holding (task keychain-prompt-loop). Without the
# cert it warns and leaves the ad-hoc signature: that build will prompt for the keychain.
# Not macOS: nothing to do.
#
# Usage: scripts/sign-if-cert.sh <binary-or-.app>...
set -euo pipefail
cd "$(dirname "${BASH_SOURCE[0]}")/.."
[ "$(uname -s)" = Darwin ] || exit 0
identity="${APPLE_SIGNING_IDENTITY:-txtodo Self-Signed}"
# Ref: https://keith.github.io/xcode-man-pages/security.1.html
if security find-identity -p codesigning | grep -qF "\"$identity\""; then
  APPLE_SIGNING_IDENTITY="$identity" scripts/macos-selfsign.sh "$@"
else
  echo "sign-if-cert: no \"$identity\" code-signing cert; $* stays ad-hoc signed, and the Keychain" \
    "will ask for it after every rebuild (import txtodo-signing.p12 to fix)" >&2
fi
