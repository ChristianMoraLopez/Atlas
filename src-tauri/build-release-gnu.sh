#!/usr/bin/env bash
# Release build for this machine (no MSVC; GNU toolchain + portable MinGW).
#
# CRITICAL: Tauri decides dev vs production mode by the `custom-protocol`
# feature of the `tauri` crate (tauri's build.rs: `let dev = !custom_protocol`).
# The Tauri CLI injects that feature automatically, but Cargo.toml keeps
# `tauri = { version = "2", features = [] }` on purpose, so a plain
# `cargo build --release` embeds the devUrl (http://127.0.0.1:1420) instead
# of the dist/ assets and the portable exe shows ERR_CONNECTION_REFUSED.
# Always pass --features tauri/custom-protocol when building with cargo directly.
set -euo pipefail

export PATH="$HOME/.cargo/bin:/d/Trabajo/Capgemini/Atlas/tmp/w64devkit-out/w64devkit/bin:$HOME/.rustup/toolchains/stable-x86_64-pc-windows-gnu/bin:$HOME/.kimi-work/bin:$PATH"

cd "$(dirname "$0")"

cargo +stable-x86_64-pc-windows-gnu build --release \
  --target x86_64-pc-windows-gnu \
  --features tauri/custom-protocol

cat <<'EOF'

Build finished. MANDATORY verification before packaging (the v0.2.24/v0.2.25
bug slipped through because verification only checked that the process did not
crash):

  cp target/x86_64-pc-windows-gnu/release/atlas-tracker.exe ../output/portable/Atlas.exe
  cd ../output/portable
  WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS=--remote-debugging-port=9229 ./Atlas.exe &
  sleep 20
  curl -s http://127.0.0.1:9229/json
  # The page MUST have url "http://tauri.localhost/" (NOT http://127.0.0.1:1420)
  # and title "Atlas — Circana Interactions Tracker". Kill the process afterwards.
EOF
