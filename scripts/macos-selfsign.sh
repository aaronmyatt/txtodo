#!/usr/bin/env bash
# Re-sign txtodo's macOS builds with the self-signed "txtodo Self-Signed" code-signing cert:
# the desktop txtodo.app (and the txtodod bundled in it), and the bare txtodo/txtodod/txtodo-tui.
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
# Usage: scripts/macos-selfsign.sh <path>...
#   <path>                  a .app bundle, or a bare binary: signed as com.txtodo.<file name>
#   APPLE_SIGNING_IDENTITY  cert common name (default "txtodo Self-Signed")
#   TXTODO_SIGN_KEYCHAIN    search only this keychain for the identity (CI's temp keychain, see
#                           scripts/macos-import-cert.sh). It must also be on the user keychain
#                           search list: codesign reports "no identity found" otherwise (tested).
set -euo pipefail

if [ "$#" -eq 0 ]; then
  echo "usage: macos-selfsign.sh <path/to/txtodo.app | path/to/binary>..." >&2
  exit 2
fi
keychain="${TXTODO_SIGN_KEYCHAIN:-}"
identity_name="${APPLE_SIGNING_IDENTITY:-txtodo Self-Signed}"

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

# No --options runtime: hardened runtime only matters for notarization, which needs Developer ID.
# Ref: https://keith.github.io/xcode-man-pages/codesign.1.html
sign() {
  codesign --force --sign "$identity" "${kc_args[@]}" "$@"
}

# Fail the build on a wrong identifier or signer instead of shipping it.
assert_signed() {
  local path="$1" id="$2" got
  got=$(codesign -dvv "$path" 2>&1)
  case "$got" in
    *"Identifier=$id"*"Authority=$identity_name"*) ;;
    *) echo "macos-selfsign: $path signed wrong:" >&2; echo "$got" >&2; exit 1 ;;
  esac
  echo "macos-selfsign: signed $path as $id"
}

for path in "$@"; do
  case "$path" in
    *.app)
      # Sign inside-out: nested code first, then the bundle, whose signature seals it all.
      sidecar="$path/Contents/MacOS/txtodod"
      if [ ! -f "$sidecar" ]; then
        echo "macos-selfsign: bundled daemon not found at $sidecar" >&2
        exit 1
      fi
      sign --identifier com.txtodo.txtodod "$sidecar"
      # The bundle's identifier comes from Info.plist's CFBundleIdentifier (com.txtodo.desktop).
      sign "$path"
      # --strict also checks the bundle layout; --deep walks the nested code signed above.
      codesign --verify --strict --deep "$path"
      assert_signed "$sidecar" com.txtodo.txtodod
      assert_signed "$path" com.txtodo.desktop
      ;;
    *)
      # Named by file name, not by release asset name: sign before the -macos-<arch> rename, so
      # the bare txtodod gets the same com.txtodo.txtodod as the bundled one.
      id="com.txtodo.$(basename "$path")"
      sign --identifier "$id" "$path"
      codesign --verify --strict "$path"
      assert_signed "$path" "$id"
      ;;
  esac
done
