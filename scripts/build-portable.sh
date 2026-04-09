#!/usr/bin/env bash
# Open Agents — Portable ZIP ビルドスクリプト
# Usage: bash scripts/build-portable.sh
set -euo pipefail

VERSION=$(grep '^version' crates/gui/Cargo.toml | head -1 | sed 's/.*"\(.*\)".*/\1/')
DIST="dist/OpenAgents-${VERSION}-portable-win-x64"
ZIP="dist/OpenAgents-${VERSION}-portable-win-x64.zip"

echo "=== Open Agents v${VERSION} — Portable Build ==="

# Clean
rm -rf "$DIST" "$ZIP"
mkdir -p "$DIST"

# 1. Main binary
echo "[1/4] Copying binary..."
cp target/release/open_agents.exe "$DIST/"

# 2. llama.cpp runtime (CUDA + Vulkan + CPU)
echo "[2/4] Copying llama.cpp runtimes..."
mkdir -p "$DIST/third_party/llama.cpp/windows-x64"
# Main dir (CUDA build + shared DLLs + manifest)
for f in third_party/llama.cpp/windows-x64/*.{dll,exe,json}; do
    [ -f "$f" ] && cp "$f" "$DIST/third_party/llama.cpp/windows-x64/"
done
# CPU subdirectory
if [ -d "third_party/llama.cpp/windows-x64/cpu" ]; then
    mkdir -p "$DIST/third_party/llama.cpp/windows-x64/cpu"
    cp third_party/llama.cpp/windows-x64/cpu/*.{dll,exe} "$DIST/third_party/llama.cpp/windows-x64/cpu/" 2>/dev/null || true
fi
# Vulkan subdirectory
if [ -d "third_party/llama.cpp/windows-x64/vulkan" ]; then
    mkdir -p "$DIST/third_party/llama.cpp/windows-x64/vulkan"
    cp third_party/llama.cpp/windows-x64/vulkan/*.{dll,exe} "$DIST/third_party/llama.cpp/windows-x64/vulkan/" 2>/dev/null || true
fi
# Upstream (if present)
if [ -d "third_party/llama.cpp/windows-x64/upstream" ]; then
    cp -r third_party/llama.cpp/windows-x64/upstream "$DIST/third_party/llama.cpp/windows-x64/"
fi

# 3. README
echo "[3/4] Copying README..."
cp README.md "$DIST/"

# 4. Create ZIP
echo "[4/4] Creating ZIP..."
cd dist
powershell.exe -Command "Compress-Archive -Path '$(basename "$DIST")' -DestinationPath '$(basename "$ZIP")' -Force"
cd ..

SIZE=$(du -sh "$ZIP" | cut -f1)
echo ""
echo "=== Done! ==="
echo "  Portable: $ZIP ($SIZE)"
echo ""
echo "使い方: ZIP を展開して open_agents.exe を実行"
