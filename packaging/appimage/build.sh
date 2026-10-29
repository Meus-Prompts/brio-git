#!/usr/bin/env bash
# Gera target/appimage/brio-git-<versão>-x86_64.AppImage.
#
#   packaging/appimage/build.sh              build com o toolchain do host
#   packaging/appimage/build.sh --container  build dentro do Debian bookworm
#                                            (glibc 2.36: roda na maioria das distros)
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
OUT="$ROOT/target/appimage"
APPDIR="$OUT/brio-git.AppDir"
TOOLS="$ROOT/target/appimage-tools"
TOOL="$TOOLS/appimagetool-x86_64.AppImage"
VERSION="${BRIO_VERSION:-$(grep -m1 '^version' "$ROOT/Cargo.toml" | cut -d'"' -f2)}"
IMAGE="docker.io/library/rust:1-bookworm"

if [[ "${1:-}" == "--container" ]]; then
    ENGINE="$(command -v podman || command -v docker || true)"
    [[ -n "$ENGINE" ]] || { echo "precisa de podman ou docker" >&2; exit 1; }
    # target separado para não misturar o cache do host com o do contêiner.
    "$ENGINE" run --rm -v "$ROOT":/src:Z -w /src -e CARGO_TARGET_DIR=/src/target/container -e BRIO_VERSION="${BRIO_VERSION:-}" \
        "$IMAGE" bash -c 'apt-get update -qq && apt-get install -y -qq file libxkbcommon-dev libwayland-dev >/dev/null && cargo build --release'
    BIN="$ROOT/target/container/release/brio-git"
else
    cargo build --release --manifest-path "$ROOT/Cargo.toml"
    BIN="$ROOT/target/release/brio-git"
fi

# AppDir limpo (o caminho é fixo dentro de target/, nunca uma pasta do usuário).
[[ "$APPDIR" == */target/appimage/brio-git.AppDir ]] && rm -rf -- "$APPDIR"
mkdir -p "$APPDIR/usr/bin" "$APPDIR/usr/share/applications" "$APPDIR/usr/share/icons/hicolor/scalable/apps"
install -m755 "$BIN" "$APPDIR/usr/bin/brio-git"
strip "$APPDIR/usr/bin/brio-git" 2>/dev/null || true
install -m755 "$ROOT/packaging/appimage/AppRun" "$APPDIR/AppRun"
install -m644 "$ROOT/packaging/brio-git.desktop" "$APPDIR/brio-git.desktop"
install -m644 "$ROOT/packaging/brio-git.desktop" "$APPDIR/usr/share/applications/brio-git.desktop"
install -m644 "$ROOT/packaging/brio-git.svg" "$APPDIR/brio-git.svg"
install -m644 "$ROOT/packaging/brio-git.svg" "$APPDIR/usr/share/icons/hicolor/scalable/apps/brio-git.svg"
ln -sf brio-git.svg "$APPDIR/.DirIcon"

if [[ ! -x "$TOOL" ]]; then
    mkdir -p "$TOOLS"
    curl -fsSL -o "$TOOL" https://github.com/AppImage/appimagetool/releases/download/continuous/appimagetool-x86_64.AppImage
    chmod +x "$TOOL"
fi

# Extract-and-run dispensa FUSE na máquina de build.
ARCH=x86_64 APPIMAGE_EXTRACT_AND_RUN=1 "$TOOL" --no-appstream "$APPDIR" "$OUT/brio-git-$VERSION-x86_64.AppImage"
echo "AppImage: $OUT/brio-git-$VERSION-x86_64.AppImage"
