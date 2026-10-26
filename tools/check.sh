#!/usr/bin/env sh
set -eu
cd "$(dirname "$0")/.."

# 初期不运行自动化测试；静态、类型、代码生成与编译检查仍须执行。
cargo fmt --all -- --check
cargo clippy --locked --workspace --lib --bins -- -D warnings
bindings_file="$(mktemp)"
trap 'rm -f "$bindings_file"' EXIT HUP INT TERM
cargo run --locked -p pixauro_editor --bin export-bindings -- "$bindings_file"
if ! cmp -s "$bindings_file" editor/src/generated/bindings.ts; then
  echo "Rust IPC 绑定已变化，请运行 cargo run --locked -p pixauro_editor --bin export-bindings 并提交生成文件。" >&2
  exit 1
fi
npm --prefix editor run lint
npm --prefix editor run format:check
npm --prefix editor run typecheck
npm --prefix editor run build
