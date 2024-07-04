#!/bin/bash

port="${PORT:-8080}"
directory="${DIRECTORY:-/data/}"

if [ "$HTTPS" = "true" ]; then
	/root/ew/ew --path $directory --port $port --https
else
	/root/ew/ew --path $directory --port $port
fi
