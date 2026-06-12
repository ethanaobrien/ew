#!/bin/bash
set -euo pipefail

args=(
  --path  "${DIRECTORY:-/data/}"
  --port  "${PORT:-8080}"
  --npps4 "${NPPS4_ADDRESS:-http://127.0.0.1:51376}"
  --max-time "${MAXTIME:-0}"
)

[ "${HTTPS:-}" = "true" ]           && args+=(--https)
[ "${HIDDEN:-}" = "true" ]          && args+=(--hidden)
[ "${PURGE:-}" = "true" ]           && args+=(--purge)
[ "${DISABLE_IMPORTS:-}" = "true" ] && args+=(--disable-imports)
[ "${DISABLE_EXPORTS:-}" = "true" ] && args+=(--disable-exports)

add_opt() {
  local value="$1" flag="$2"
  if [ -n "$value" ]; then
    args+=("$flag" "$value")
  fi
}

# Asset hash / version overrides
add_opt "${JP_ANDROID_ASSET_HASH:-}" --jp-android-asset-hash
add_opt "${JP_IOS_ASSET_HASH:-}"     --jp-ios-asset-hash
add_opt "${EN_ANDROID_ASSET_HASH:-}" --en-android-asset-hash
add_opt "${EN_IOS_ASSET_HASH:-}"     --en-ios-asset-hash
add_opt "${WINDOWS_ASSET_VERSION:-}" --windows-asset-version
add_opt "${WINDOWS_ASSET_HASH:-}"    --windows-asset-hash

# Asset / image paths.
add_opt "${IMAGE_ASSET_PATH:-}" --image-asset-path
add_opt "${MASTERDATA:-}"       --masterdata

# "Help" page download links + asset server.
add_opt "${ANDROID_GLOBAL:-}" --global-android
add_opt "${ANDROID_JAPAN:-}"  --japan-android
add_opt "${IOS_GLOBAL:-}"     --global-ios
add_opt "${IOS_JAPAN:-}"      --japan-ios
add_opt "${ASSET_URL:-}"      --assets-url

exec /root/ew/ew "${args[@]}"
