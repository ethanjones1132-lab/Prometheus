#!/bin/bash
# Kill old processes
/mnt/c/Windows/System32/taskkill.exe /IM "kalshi-monster.exe" /F 2>/dev/null
/mnt/c/Windows/System32/taskkill.exe /IM "cargo.exe" /F 2>/dev/null
/mnt/c/Windows/System32/taskkill.exe /IM "rustc.exe" /F 2>/dev/null
pkill -f "vite" 2>/dev/null
sleep 3

# Launch tauri dev using Windows cargo-tauri
export PATH="/mnt/c/Users/ethan/.cargo/bin:$PATH"
cd /home/ethan/.openclaw/agents/coderclaw/workspace/kalshi-monster

nohup /mnt/c/Users/ethan/.cargo/bin/cargo.exe tauri dev > /tmp/kalshi-tauri-dev.log 2>&1 &
echo "Launched PID: $!"
