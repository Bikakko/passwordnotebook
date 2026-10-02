#!/usr/bin/env bash
#
# 在 WSL 中交叉编译 Windows 原生程序。
# 前置条件(WSL 内,一次性):
#   apt-get install -y gcc-mingw-w64-x86-64
#   rustup target add x86_64-pc-windows-gnu
#
set -euo pipefail

cd "$(dirname "${BASH_SOURCE[0]}")"

# Rust 已是系统级安装(见 /etc/profile.d/rust.sh):cargo 在 /usr/local/bin,
# 工具链在 /usr/local/rustup。登录 shell 会自动注入 RUSTUP_HOME,这里为
# 未经过 profile 的环境(非登录 shell、脚本调用)兜底。
export RUSTUP_HOME="${RUSTUP_HOME:-/usr/local/rustup}"

# 可复现构建:PE 头会写入构建时间戳,同样的源码每次产出的字节都不同。
# 零掉时间戳,让同源码 = 同产物(便于比对「部署的那份到底是哪次构建」)。
export RUSTFLAGS="${RUSTFLAGS-} -C link-arg=-Wl,--no-insert-timestamp"

cargo build --release --target x86_64-pc-windows-gnu

EXE="target/x86_64-pc-windows-gnu/release/pnb.exe"
echo
echo "已生成:$EXE"
ls -la "$EXE"
echo
echo "运行:把该 exe 拷到 Windows 盘符上的任意目录(UNC 路径下无法直接运行),双击即可。"
