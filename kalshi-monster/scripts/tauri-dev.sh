#!/bin/bash
# Launch Tauri dev server using Windows-side cargo-tauri via WSL2 interop
export PATH="/mnt/c/Users/ethan/.cargo/bin:$PATH"

# WSL2 software rendering fallback (bypasses ZINK/EGL GPU failures)
export WEBKIT_DISABLE_COMPOSITING_MODE=1
export LIBGL_ALWAYS_SOFTWARE=1
export MESA_GL_VERSION_OVERRIDE=3.3

cd /home/ethan/.openclaw/agents/coderclaw/workspace/kalshi-monster

echo "[kalshi-monster] Starting Tauri dev server..."
echo "[kalshi-monster] cargo-tauri: $(cargo-tauri --version 2>/dev/null || echo 'not found')"

cargo tauri dev
