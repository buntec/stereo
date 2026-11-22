#!/usr/bin/env bash
# Notarizes a file (a zip, .dmg or .pkg) with Apple, failing unless Apple accepts it.
# notarytool's exit status doesn't tell a rejection apart from success, so this checks the status.
# Needs APPLE_API_KEY (the key ID), APPLE_API_ISSUER and APPLE_API_KEY_PATH (the .p8 file).
set -euo pipefail

file="$1"
auth=(--key "$APPLE_API_KEY_PATH" --key-id "$APPLE_API_KEY" --issuer "$APPLE_API_ISSUER")

# Usually minutes, but Apple sometimes takes hours.
result="$(xcrun notarytool submit "$file" --wait --timeout 3h --output-format json "${auth[@]}")"
echo "$result"
id="$(plutil -extract id raw - <<< "$result")"
status="$(plutil -extract status raw - <<< "$result")"
if [ "$status" != Accepted ]; then
  xcrun notarytool log "$id" "${auth[@]}" || true
  echo "::error::Notarization of $file: $status"
  exit 1
fi
