#!/usr/bin/env bash
# Builds target/macos/brio-git-<version>-macos-universal.zip: a brio-git.app
# with one binary for Apple Silicon and Intel. Runs on macOS (needs lipo,
# iconutil and sips from Xcode, and rsvg-convert from `brew install librsvg`).
#
#   packaging/macos/bundle.sh
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
OUT="$ROOT/target/macos"
APP="$OUT/brio-git.app"
VERSION="$(grep -m1 '^version' "$ROOT/Cargo.toml" | cut -d'"' -f2)"

for target in aarch64-apple-darwin x86_64-apple-darwin; do
    cargo build --release -p brio-git --target "$target" --manifest-path "$ROOT/Cargo.toml"
done

# Fresh bundle (the path is fixed under target/, never a user directory).
[[ "$APP" == */target/macos/brio-git.app ]] && rm -rf -- "$APP"
mkdir -p "$APP/Contents/MacOS" "$APP/Contents/Resources"
lipo -create -output "$APP/Contents/MacOS/brio-git" \
    "$ROOT/target/aarch64-apple-darwin/release/brio-git" \
    "$ROOT/target/x86_64-apple-darwin/release/brio-git"
sed "s/@VERSION@/$VERSION/g" "$ROOT/packaging/macos/Info.plist" >"$APP/Contents/Info.plist"

# Icon: the SVG rendered at every size macOS asks for.
ICONSET="$OUT/brio-git.iconset"
rm -rf -- "$ICONSET"
mkdir -p "$ICONSET"
for size in 16 32 128 256 512; do
    rsvg-convert -w "$size" -h "$size" "$ROOT/packaging/brio-git.svg" -o "$ICONSET/icon_${size}x${size}.png"
    rsvg-convert -w "$((size * 2))" -h "$((size * 2))" "$ROOT/packaging/brio-git.svg" -o "$ICONSET/icon_${size}x${size}@2x.png"
done
iconutil -c icns "$ICONSET" -o "$APP/Contents/Resources/brio-git.icns"

ZIP="$OUT/brio-git-$VERSION-macos-universal.zip"
rm -f -- "$ZIP"
ditto -c -k --keepParent "$APP" "$ZIP"
echo "macOS: $ZIP"
