#!/usr/bin/env bash
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
RUNTIME_ROOT="${1:-$REPO_ROOT/runtime}"
BACKEND="${LOCAL_AI_BACKEND:-auto}"

BIN_DIR="$RUNTIME_ROOT/bin"
MODELS_DIR="$RUNTIME_ROOT/models"
CACHE_DIR="$RUNTIME_ROOT/cache"
CONFIG_PATH="$REPO_ROOT/config.toml"

mkdir -p "$BIN_DIR" "$MODELS_DIR" "$CACHE_DIR"

OS_RAW="$(uname -s)"
ARCH_RAW="$(uname -m)"

case "$OS_RAW" in
  Darwin) TARGET_OS="macos" ;;
  Linux)  TARGET_OS="linux" ;;
  *) echo "Unsupported OS: $OS_RAW"; exit 1 ;;
esac

case "$ARCH_RAW" in
  arm64|aarch64) TARGET_ARCH="arm64" ;;
  x86_64|amd64)  TARGET_ARCH="x64" ;;
  *) echo "Unsupported Arch: $ARCH_RAW"; exit 1 ;;
esac

CUDA_MAJOR=12
CUDA_MINOR=4

if [ "$BACKEND" = "auto" ]; then
  if [ "$TARGET_OS" = "macos" ] && [ "$TARGET_ARCH" = "arm64" ]; then
    BACKEND="metal"
  elif [ "$TARGET_OS" = "macos" ]; then
    BACKEND="cpu"
  elif command -v nvidia-smi >/dev/null 2>&1; then
    SMI_VER="$(nvidia-smi | sed -n 's/.*CUDA Version: \([0-9]*\)\.\([0-9]*\).*/\1 \2/p' | head -n1 || true)"
    if [ -n "$SMI_VER" ]; then
      CUDA_MAJOR="$(echo "$SMI_VER" | awk '{print $1}')"
      CUDA_MINOR="$(echo "$SMI_VER" | awk '{print $2}')"
    fi
    if [ "$CUDA_MAJOR" -ge 13 ]; then
      BACKEND="cuda13"
    elif [ "$CUDA_MAJOR" -eq 12 ] && [ "$CUDA_MINOR" -ge 8 ]; then
      BACKEND="cuda12.8"
    else
      BACKEND="cuda12.4"
    fi
  elif command -v rocminfo >/dev/null 2>&1 || grep -qi "0x1002" /sys/class/drm/card*/device/vendor 2>/dev/null; then
    BACKEND="rocm"
  elif grep -qi "0x8086" /sys/class/drm/card*/device/vendor 2>/dev/null; then
    BACKEND="sycl"
  elif ldconfig -p 2>/dev/null | grep -q "libvulkan.so.1" || command -v vulkaninfo >/dev/null 2>&1; then
    BACKEND="vulkan"
  else
    BACKEND="cpu"
  fi
fi

echo "=================================================================="
echo "  PrismML Ternary-Bonsai-2.5-27B-GGUF Plug-and-Play Bootstrapper"
echo "  Platform : $TARGET_OS-$TARGET_ARCH | Backend: $BACKEND"
echo "  Runtime  : $RUNTIME_ROOT"
echo "=================================================================="

RELEASES_JSON="$(curl -fsSL -H "User-Agent: LocalAI-Companion-Setup" "https://api.github.com/repos/PrismML-Eng/llama.cpp/releases?per_page=6")"

SELECT_URL="$(python3 - "$TARGET_OS" "$TARGET_ARCH" "$BACKEND" <<'PYEOF' <<< "$RELEASES_JSON"
import json, sys
os_name, arch, backend = sys.argv[1], sys.argv[2], sys.argv[3]
releases = json.load(sys.stdin)

def pick(assets):
    names = {a["name"]: a["browser_download_url"] for a in assets if a.get("name","").endswith((".tar.gz", ".zip"))}
    def find_match(inc, exc=()):
        for n, u in names.items():
            low = n.lower()
            if all(i in low for i in inc) and not any(e in low for e in exc):
                return n, u
        return None
    if os_name == "macos" and arch == "arm64":
        return find_match(["bin-macos-arm64.tar.gz"], ["kleidiai"]) or find_match(["bin-macos-arm64"])
    if os_name == "macos":
        return find_match(["bin-macos-x64.tar.gz"])
    if os_name == "linux" and arch == "x64":
        if backend == "cuda13":
            return find_match(["bin-linux-cuda-13", "-x64"]) or find_match(["bin-linux-cuda-12.8", "-x64"]) or find_match(["bin-linux-cuda-12.4", "-x64"])
        if backend == "cuda12.8":
            return find_match(["bin-linux-cuda-12.8", "-x64"]) or find_match(["bin-linux-cuda-12.4", "-x64"])
        if backend.startswith("cuda"):
            return find_match(["bin-linux-cuda-12.4", "-x64"])
        if backend == "rocm":
            return find_match(["bin-ubuntu-rocm", "-x64"]) or find_match(["bin-ubuntu-vulkan-x64"])
        if backend == "sycl":
            return find_match(["bin-ubuntu-sycl-fp16-x64"]) or find_match(["bin-ubuntu-vulkan-x64"])
        if backend == "vulkan":
            return find_match(["bin-ubuntu-vulkan-x64"]) or find_match(["bin-ubuntu-x64.tar.gz"])
        return find_match(["bin-ubuntu-x64.tar.gz"])
    if os_name == "linux" and arch == "arm64":
        if backend == "vulkan":
            return find_match(["bin-ubuntu-vulkan-arm64.tar.gz"]) or find_match(["bin-ubuntu-arm64.tar.gz"])
        return find_match(["bin-ubuntu-arm64.tar.gz"])
    return None

for rel in releases:
    res = pick(rel.get("assets", []))
    if res:
        print(f"{rel.get('tag_name','')}|{res[0]}|{res[1]}")
        break
PYEOF
)"

if [ -z "$SELECT_URL" ]; then
  echo "Error: No matching pre-built PrismML archive found for $TARGET_OS-$TARGET_ARCH ($BACKEND)."
  exit 1
fi

REL_TAG="$(echo "$SELECT_URL" | cut -d'|' -f1)"
ASSET_NAME="$(echo "$SELECT_URL" | cut -d'|' -f2)"
ASSET_URL="$(echo "$SELECT_URL" | cut -d'|' -f3)"

echo "[1/2] Downloading $ASSET_NAME ($REL_TAG) ..."
ARCHIVE_PATH="$CACHE_DIR/$ASSET_NAME"
if [ ! -s "$ARCHIVE_PATH" ]; then
  curl -fL -C - --retry 5 --retry-delay 2 --progress-bar "$ASSET_URL" -o "$ARCHIVE_PATH.part"
  mv "$ARCHIVE_PATH.part" "$ARCHIVE_PATH"
fi

TMP_EXTRACT="$CACHE_DIR/extract_tmp"
rm -rf "$TMP_EXTRACT"
mkdir -p "$TMP_EXTRACT"
tar -xzf "$ARCHIVE_PATH" -C "$TMP_EXTRACT"

find "$TMP_EXTRACT" -type f \( -name "llama-server" -o -name "*.so*" -o -name "*.dylib" \) -exec cp -P {} "$BIN_DIR/" \;
chmod +x "$BIN_DIR/llama-server" 2>/dev/null || true
rm -rf "$TMP_EXTRACT"

echo "[2/2] Checking Hugging Face model files ..."
MODEL_FILE="Ternary-Bonsai-2.5-27B-Q2_0.gguf"
MMPROJ_FILE="$(find "$MODELS_DIR" -maxdepth 1 -name "*mmproj*.gguf" -print -quit || true)"

if [ ! -s "$MODELS_DIR/$MODEL_FILE" ] || [ -z "$MMPROJ_FILE" ]; then
  HF_JSON="$(curl -fsSL "https://huggingface.co/api/models/prism-ml/Ternary-Bonsai-2.5-27B-GGUF")"
  MMPROJ_NAME="$(python3 -c 'import json,sys; d=json.load(sys.stdin); print(next((s["rfilename"] for s in d.get("siblings",[]) if "mmproj" in s.get("rfilename","").lower() and s.get("rfilename","").endswith(".gguf")), ""))' <<< "$HF_JSON")"

  for F in "$MODEL_FILE" "$MMPROJ_NAME"; do
    [ -z "$F" ] && continue
    DEST="$MODELS_DIR/$(basename "$F")"
    if [ ! -s "$DEST" ]; then
      echo "Downloading $F ..."
      curl -fL -C - --progress-bar "https://huggingface.co/prism-ml/Ternary-Bonsai-2.5-27B-GGUF/resolve/main/$F" -o "$DEST"
    fi
  done
  MMPROJ_FILE="$(find "$MODELS_DIR" -maxdepth 1 -name "*mmproj*.gguf" -print -quit || true)"
fi

MMPROJ_CFG=""
if [ -n "$MMPROJ_FILE" ]; then
  MMPROJ_CFG="runtime/models/$(basename "$MMPROJ_FILE")"
fi

cat > "$CONFIG_PATH" <<EOF
[runtime]
root_dir = "runtime"
llama_server_bin = "runtime/bin/llama-server"
model_path = "runtime/models/$MODEL_FILE"
mmproj_path = "$MMPROJ_CFG"

[llm]
backend = "auto"
auto_vram_sizing = true
host = "127.0.0.1"
port = 8080
n_gpu_layers = 99
ctx_size = 8192
batch_size = 512
ubatch_size = 64
parallel = 1
EOF

echo "Setup complete! llama-server installed at $BIN_DIR/llama-server"
