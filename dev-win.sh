#!/usr/bin/env bash
set -euo pipefail

cd "$(dirname "$0")"

# 1. Start vite dev server in background
echo ":: starting vite dev server …"
npm run dev -- --host &
VITE_PID=$!
trap "kill $VITE_PID 2>/dev/null" EXIT

# 2. Wait for vite to be ready
echo ":: waiting for vite on :1420 …"
for i in $(seq 1 30); do
  if curl -s http://localhost:1420 >/dev/null 2>&1; then
    break
  fi
  sleep 0.5
done

# 3. Cross-compile Rust for Windows (debug, no custom-protocol → uses devUrl)
echo ":: building Rust for Windows (debug) …"
cd src-tauri
cargo xwin build --target x86_64-pc-windows-msvc --no-default-features

# 4. Launch the .exe (WSL can run Windows executables)
EXE="target/x86_64-pc-windows-msvc/debug/lv.exe"
echo ":: launching $EXE …"
"$EXE" "$@"
