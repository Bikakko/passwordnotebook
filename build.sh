#!/usr/bin/env bash
#
# 在 WSL 中交叉编译 Windows 原生程序。
# 前置条件(WSL 内,一次性):
#   apt-get install -y gcc-mingw-w64-x86-64
#   rustup target add x86_64-pc-windows-gnu
#
set -euo pipefail

cd "$(dirname "${BASH_SOURCE[0]}")"
export PATH="$HOME/.cargo/bin:$PATH"

cargo build --release --target x86_64-pc-windows-gnu

EXE="target/x86_64-pc-windows-gnu/release/PasswordNotebook.exe"
echo
echo "已生成:$EXE"
ls -la "$EXE"
echo
echo "运行:把该 exe 拷到 Windows 盘符上的任意目录(UNC 路径下无法直接运行),双击即可。"
