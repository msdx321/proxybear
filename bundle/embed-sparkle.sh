#!/bin/sh
# Download the pinned Sparkle release and embed its framework in an app bundle.
# The extracted release, including bin/sign_update, stays in the tools directory.
#
# usage: bundle/embed-sparkle.sh path/to/ProxyBear.app [tools-dir]
set -eu

SPARKLE_VERSION=2.10.0
SPARKLE_SHA256=c2bf58aa8387266ac179357b1415d6f2635f044da8be41042af32425dae6da0c

app="$1"
tools="${2:-target/sparkle}"
archive="$tools/Sparkle-$SPARKLE_VERSION.tar.xz"

mkdir -p "$tools"
if [ ! -f "$archive" ]; then
  curl -fsSL -o "$archive" \
    "https://github.com/sparkle-project/Sparkle/releases/download/$SPARKLE_VERSION/Sparkle-$SPARKLE_VERSION.tar.xz"
fi
echo "$SPARKLE_SHA256  $archive" | shasum -a 256 -c - >/dev/null
tar -xf "$archive" -C "$tools"

mkdir -p "$app/Contents/Frameworks"
rm -rf "$app/Contents/Frameworks/Sparkle.framework"
ditto "$tools/Sparkle.framework" "$app/Contents/Frameworks/Sparkle.framework"
