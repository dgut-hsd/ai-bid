#!/usr/bin/env bash
# 为 jwcai 宿主机增加 swap，缓解「7.7G 内存 + 0 swap」下的 OOM。
# 背景：全量构建（Maven/Cargo/Vite）时 buildkit 与 rustc 内存峰值过大，
#       2026-08-26/27 已两次触发 oom-killer。
# 用法: sudo bash add-swap.sh [大小]     # 默认 4G；幂等，可重复运行
set -euo pipefail

[ "$EUID" -eq 0 ] || { echo "请用 sudo 运行: sudo bash $0" >&2; exit 1; }

SIZE="${1:-4G}"
SWAPFILE="${SWAPFILE:-/swapfile}"
SWAPPINESS="${SWAPPINESS:-10}"

# 幂等：已启用同名 swapfile 则跳过
if swapon --show | awk '{print $1}' | grep -q "^${SWAPFILE}$"; then
    echo "swapfile 已启用，跳过。当前状态："
    swapon --show
    free -h
    exit 0
fi

echo "==> 创建 swapfile（$SIZE）: $SWAPFILE"
if [ ! -f "$SWAPFILE" ]; then
    fallocate -l "$SIZE" "$SWAPFILE"
fi
chmod 600 "$SWAPFILE"

echo "==> 格式化并启用"
mkswap "$SWAPFILE" >/dev/null
swapon "$SWAPFILE"

echo "==> 写入 /etc/fstab 持久化"
if ! grep -q '^/swapfile' /etc/fstab; then
    echo '/swapfile none swap sw 0 0' >> /etc/fstab
fi

echo "==> 调整 swappiness=$SWAPPINESS（降低内核优先杀进程的倾向）"
sysctl -w vm.swappiness="$SWAPPINESS" >/dev/null
if ! grep -q '^vm.swappiness' /etc/sysctl.conf; then
    echo "vm.swappiness=$SWAPPINESS" >> /etc/sysctl.conf
fi

echo "==> 完成："
swapon --show
free -h