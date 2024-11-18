#!/bin/bash

port="${PORT:-8080}"
directory="${DIRECTORY:-/data/}"

npps4="${NPPS4_ADDRESS:-http://127.0.0.1:51376}"

https=$([ "$HTTPS" = "true" ] && echo "--https" || echo "")

hidden=$([ "$HIDDEN" = "true" ] && echo "--hidden" || echo "")

maxTime="${MAXTIME:-0}"

purge=$([ "$PURGE" = "true" ] && echo "--purge" || echo "")

imports=$([ "$DISABLE_IMPORTS" = "true" ] && echo "--disable-imports" || echo "")

exports=$([ "$DISABLE_EXPORTS" = "true" ] && echo "--disable-exports" || echo "")

asset_android_jp=$([ "$JP_ANDROID_ASSET_HASH" != "" ] && echo "--jp-android-asset-hash $JP_ANDROID_ASSET_HASH" || echo "")

asset_ios_jp=$([ "$JP_IOS_ASSET_HASH" != "" ] && echo "--jp-ios-asset-hash $JP_IOS_ASSET_HASH" || echo "")

asset_android_en=$([ "$EN_ANDROID_ASSET_HASH" != "" ] && echo "--en-android-asset-hash $EN_ANDROID_ASSET_HASH" || echo "")

asset_ios_en=$([ "$EN_IOS_ASSET_HASH" != "" ] && echo "--en-ios-asset-hash $EN_IOS_ASSET_HASH" || echo "")

/root/ew/ew --path $directory --port $port --npps4 $npps4 $asset_android_jp $asset_ios_jp $asset_android_en $asset_ios_en $exports $imports $purge $hidden $https --global-android "$ANDROID_GLOBAL"  --japan-android "$ANDROID_JAPAN"  --global-ios "$IOS_GLOBAL"  --japan-ios "$IOS_JAPAN" --assets-url "$ASSET_URL" --max-time $maxTime
