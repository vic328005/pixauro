#!/usr/bin/env sh
set -eu
cd "$(dirname "$0")/.."

# 用锁定版本的 Tauri CLI 渲染正式 SVG，仅保留桌面应用所需的图标。
generated_icons="$(mktemp -d)"
trap 'rm -rf "$generated_icons"' EXIT HUP INT TERM
editor/node_modules/.bin/tauri icon editor/src-tauri/icons/pixauro-app-icon.svg \
  --output "$generated_icons"

for icon in 32x32.png 128x128.png 128x128@2x.png icon.png icon.ico icon.icns; do
  cp "$generated_icons/$icon" "editor/src-tauri/icons/$icon"
done
