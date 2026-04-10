#!/usr/bin/env bash
# Install the locally-built llama-cpp-turboquant (RotorQuant) binaries into third_party/llama.cpp/windows-x64
set -euo pipefail

SRC="D:/NEXTCLOUD/Windows_app/Open_Agents/.tmp/llama-cpp-turboquant"
DST="D:/NEXTCLOUD/Windows_app/Open_Agents/third_party/llama.cpp/windows-x64"

taskkill.exe /F /IM open_agents.exe 2>/dev/null || true
taskkill.exe /F /IM llama-server.exe 2>/dev/null || true

echo "[1/5] Installing CUDA build to root..."
# Keep the cudart/cublas DLLs from the existing install (CUDA Toolkit runtimes)
for f in "$SRC/build-cuda/bin"/*.{dll,exe}; do
    name=$(basename "$f")
    cp "$f" "$DST/$name"
done

echo "[2/5] Installing CPU build to cpu/..."
mkdir -p "$DST/cpu"
# Remove old CPU binaries but preserve unrelated files
for f in "$DST/cpu"/*.{dll,exe}; do
    [ -f "$f" ] && rm -f "$f"
done
for f in "$SRC/build-cpu/bin"/*.{dll,exe}; do
    cp "$f" "$DST/cpu/$(basename "$f")"
done

echo "[3/5] Installing Vulkan build to vulkan/..."
mkdir -p "$DST/vulkan"
for f in "$DST/vulkan"/*.{dll,exe}; do
    [ -f "$f" ] && rm -f "$f"
done
for f in "$SRC/build-vulkan/bin"/*.{dll,exe}; do
    cp "$f" "$DST/vulkan/$(basename "$f")"
done

echo "[4/5] Installing CUDA to upstream/cuda/..."
mkdir -p "$DST/upstream/cuda"
for f in "$DST/upstream/cuda"/*.{dll,exe}; do
    [ -f "$f" ] && rm -f "$f"
done
for f in "$SRC/build-cuda/bin"/*.{dll,exe}; do
    cp "$f" "$DST/upstream/cuda/$(basename "$f")"
done
# Re-copy CUDA runtime DLLs from source (cudart, cublas, cublasLt)
for name in cudart64_13.dll cublas64_13.dll cublasLt64_13.dll; do
    if [ -f "$DST/$name.bak" ]; then
        cp "$DST/$name.bak" "$DST/$name"
        cp "$DST/$name.bak" "$DST/upstream/cuda/$name"
    fi
done

echo "[5/5] Installing CPU to upstream/cpu/..."
mkdir -p "$DST/upstream/cpu"
for f in "$DST/upstream/cpu"/*.{dll,exe}; do
    [ -f "$f" ] && rm -f "$f"
done
for f in "$SRC/build-cpu/bin"/*.{dll,exe}; do
    cp "$f" "$DST/upstream/cpu/$(basename "$f")"
done

echo "Done."
