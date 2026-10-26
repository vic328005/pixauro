#!/bin/zsh
set -eu
cd -- "$(dirname -- "$0")" || exit 1
export PATH="${CARGO_HOME:-$HOME/.cargo}/bin:$PATH"

if ! command -v npm >/dev/null 2>&1; then
  echo "请先安装 Node.js LTS，然后运行 npm --prefix editor ci。"
  exit 1
fi
if [ ! -d editor/node_modules ]; then
  npm --prefix editor ci
fi
exec npm --prefix editor run desktop
