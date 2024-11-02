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

/root/ew/ew --path $directory --port $port --npps4 $npps4 $exports $imports $purge $hidden $https --global-android "$ANDROID_GLOBAL"  --japan-android "$ANDROID_JAPAN"  --global-ios "$IOS_GLOBAL"  --japan-ios "$IOS_JAPAN" --assets-url "$ASSET_URL" --max-time $maxTime
