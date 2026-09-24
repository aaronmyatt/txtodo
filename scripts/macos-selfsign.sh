#!/usr/bin/env bash
# Re-sign a built txtodo.app with the self-signed "txtodo Self-Signed" code-signing cert.
#
# Why: the macOS Keychain saves "Always Allow" against a binary's designated requirement, meaning
# its signing identifier plus its cert. Ad-hoc builds get a new content-hash identifier
# (txtodod-<hash>) every build, so every release re-prompts for the keychain password. A fixed
# --identifier plus one long-lived cert keeps the requirement the same across releases.
# Ref: https://developer.apple.com/library/archive/documentation/Security/Conceptual/CodeSigningGuide/RequirementLang/RequirementLang.html
#
# Why not Tauri's own signing: its APPLE_CERTIFICATE import only looks for Apple-issued certs
# ("Developer ID Application:", "Apple Development:", ...), and its codesign call passes no
# --identifier, so the bundled txtodod would keep the per-build hash identifier.
# Ref: https://github.com/tauri-apps/tauri/blob/dev/crates/tauri-macos-sign/src/keychain/identity.rs
#
# Usage: scripts/macos-selfsign.sh <path/to/txtodo.app> [keychain]
#   APPLE_SIGNING_IDENTITY  cert common name (default "txtodo Self-Signed")
#   keychain                search only this keychain for the identity (CI's temp keychain).
#                           It must also be on the user keychain search list: codesign reports
#                           "no identity found" otherwise, even with --keychain (tested).
set -euo pipefail

app="${1:?usage: macos-selfsign.sh <path/to/txtodo.app> [keychain]}"
keychain="${2:-}"
identity_name="${APPLE_SIGNING_IDENTITY:-txtodo Self-Signed}"
daemon_id="com.txtodo.txtodod"

# Resolve the cert name to its SHA-1 hash. No -v: a self-signed cert is untrusted on a CI runner,
# so -v (valid only) would hide it, but codesign still signs with it by hash.
# Ref: https://keith.github.io/xcode-man-pages/security.1.html
# shellcheck disable=SC2086 # $keychain is empty or one path; empty must add no argument
identity=$(security find-identity -p codesigning ${keychain:+"$keychain"} \
  | awk -v name="\"$identity_name\"" 'index($0, name) {print $2; exit}')
if [ -z "$identity" ]; then
  echo "macos-selfsign: no code-signing identity named \"$identity_name\" found" >&2
  exit 1
fi

kc_args=()
if [ -n "$keychain" ]; then kc_args=(--keychain "$keychain"); fi

# Sign inside-out: nested code first, then the bundle, whose signature seals everything in it.
# No --options runtime: hardened runtime only matters for notarization, which needs Developer ID.
# Ref: https://keith.github.io/xcode-man-pages/codesign.1.html
sidecar="$app/Contents/MacOS/txtodod"
if [ ! -f "$sidecar" ]; then
  echo "macos-selfsign: bundled daemon not found at $sidecar" >&2
  exit 1
fi
codesign --force --sign "$identity" "${kc_args[@]}" --identifier "$daemon_id" "$sidecar"
# The bundle's own identifier comes from Info.plist's CFBundleIdentifier (com.txtodo.desktop).
codesign --force --sign "$identity" "${kc_args[@]}" "$app"

# --strict also checks the bundle layout; --deep walks the nested code we just signed.
codesign --verify --strict --deep "$app"

# Assert the identifier and signer, so a regression fails the build instead of shipping.
got=$(codesign -dvv "$sidecar" 2>&1)
case "$got" in
  *"Identifier=$daemon_id"*"Authority=$identity_name"*) ;;
  *) echo "macos-selfsign: $sidecar signed wrong:" >&2; echo "$got" >&2; exit 1 ;;
esac
echo "macos-selfsign: signed $app as \"$identity_name\" (daemon identifier $daemon_id)"
