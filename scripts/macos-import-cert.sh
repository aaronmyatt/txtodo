#!/usr/bin/env bash
# CI only: import the self-signed signing cert into a throwaway keychain for this job, so
# scripts/macos-selfsign.sh can sign with it. Follows GitHub's recipe for macOS runners.
# Ref: https://docs.github.com/en/actions/use-cases-and-examples/deploying/installing-an-apple-certificate-on-macos-runners-for-xcode-development
#
# Env: APPLE_CERTIFICATE (base64 .p12), APPLE_CERTIFICATE_PASSWORD, RUNNER_TEMP, GITHUB_ENV.
# Writes TXTODO_SIGN_KEYCHAIN to $GITHUB_ENV for later steps. Remove with:
#   security delete-keychain "$TXTODO_SIGN_KEYCHAIN"
set -euo pipefail

: "${APPLE_CERTIFICATE:?missing secret APPLE_CERTIFICATE}"
: "${APPLE_CERTIFICATE_PASSWORD:?missing secret APPLE_CERTIFICATE_PASSWORD}"

keychain="$RUNNER_TEMP/txtodo-signing.keychain-db"
keychain_password="$(openssl rand -hex 24)"
p12="$RUNNER_TEMP/cert.p12"

echo "$APPLE_CERTIFICATE" | base64 --decode > "$p12"
# Ref: https://keith.github.io/xcode-man-pages/security.1.html
security create-keychain -p "$keychain_password" "$keychain"
security set-keychain-settings -lut 21600 "$keychain"
security unlock-keychain -p "$keychain_password" "$keychain"
security import "$p12" -P "$APPLE_CERTIFICATE_PASSWORD" -A -t cert -f pkcs12 -k "$keychain"
rm "$p12"
# Lets codesign use the key without a GUI "allow access" prompt.
security set-key-partition-list -S apple-tool:,apple:,codesign: -s -k "$keychain_password" "$keychain" >/dev/null
# codesign only finds identities in keychains on the search list, even with --keychain.
# shellcheck disable=SC2046 # word-split the existing list, one keychain path per word
security list-keychains -d user -s "$keychain" $(security list-keychains -d user | tr -d '"')

echo "TXTODO_SIGN_KEYCHAIN=$keychain" >> "$GITHUB_ENV"
echo "macos-import-cert: imported into $keychain"
