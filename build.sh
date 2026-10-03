#!/usr/bin/env bash
# ============================================================
#  Build obsidian-sync (single-file executable, Ubuntu/Linux)
#  Output: bin/obsidian-sync
# ============================================================
set -euo pipefail
cd "$(dirname "$0")"

if ! command -v cargo >/dev/null 2>&1; then
  echo "[error] cargo not found. Install the Rust toolchain first: https://rustup.rs"
  echo "        e.g. curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y"
  exit 1
fi

TARGET_ARGS=()
OUT="target/release/obsidian-sync"

if ! command -v cc >/dev/null 2>&1 && ! command -v gcc >/dev/null 2>&1; then
  # 系统没有 cc/gcc（只装了 rustup 的典型情况）：
  # 改用 rustup 自带的 rust-lld 在 musl 目标上链接，产出静态单文件可执行。
  echo "[info] system cc/gcc not found, building static musl binary with bundled rust-lld"
  rustup target add x86_64-unknown-linux-musl
  sysroot="$(rustc --print sysroot)"
  tmpbin="$(mktemp -d)"
  trap 'rm -rf "$tmpbin"' EXIT
  ln -sf "$sysroot/lib/rustlib/x86_64-unknown-linux-gnu/bin/rust-lld" "$tmpbin/ld.lld"
  export RUSTFLAGS="-C linker=$tmpbin/ld.lld -C linker-flavor=ld.lld -C link-self-contained=yes"
  TARGET_ARGS=(--target x86_64-unknown-linux-musl)
  OUT="target/x86_64-unknown-linux-musl/release/obsidian-sync"
fi

echo "== running unit tests =="
cargo test --quiet "${TARGET_ARGS[@]}"

echo "== building release =="
cargo build --release "${TARGET_ARGS[@]}"

mkdir -p bin
cp -f "$OUT" bin/obsidian-sync

echo
echo "Built: $(pwd)/bin/obsidian-sync"
echo
echo "Common commands:"
echo "  bin/obsidian-sync gui              launch the GUI"
echo "  bin/obsidian-sync doctor           check environment and vaults"
echo "  bin/obsidian-sync new ../NewVault  add a new vault"
echo "  bin/obsidian-sync check            health check only"
