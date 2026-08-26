#!/usr/bin/env bash
# 自动更新：拉取 main 最新代码 → 重新构建 → 滚动更新容器。
# 用法：./update.sh          正常更新
#       ./update.sh --force  强制更新（清除回滚保持状态）

set -euo pipefail

DEPLOY_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
APP_DIR="$(dirname "$DEPLOY_DIR")"
HISTORY_FILE="$DEPLOY_DIR/.deploy-history"
HOLD_FILE="$DEPLOY_DIR/.rollback-hold"

# --force：忽略回滚保持状态
if [ "${1:-}" = "--force" ]; then
    rm -f "$HOLD_FILE"
    echo "==> 强制更新模式（已清除回滚保持标记）"
fi

# 处于回滚保持状态时，避免自动更新把回滚版本覆盖掉
if [ -f "$HOLD_FILE" ]; then
    echo "⏸️  当前处于「回滚保持」状态，暂不自动更新。"
    echo "   排查完问题后：删除 $HOLD_FILE 或执行 ./update.sh --force 恢复追 main。"
    exit 0
fi

cd "$APP_DIR"

echo "==> 切回 main 分支并检查更新..."
git checkout main
git fetch origin main

LOCAL=$(git rev-parse HEAD)
REMOTE=$(git rev-parse origin/main)

if [ "$LOCAL" = "$REMOTE" ]; then
    echo "✅ 已是最新版本 $(git rev-parse --short HEAD)，无需更新"
    exit 0
fi

echo "==> 发现新版本：$(git rev-parse --short "$LOCAL") → $(git rev-parse --short "$REMOTE")"
git pull --ff-only origin main
NEW=$(git rev-parse HEAD)

cd "$DEPLOY_DIR"

echo "==> 重新构建并滚动更新（数据卷不动）..."
if docker compose up -d --build --remove-orphans; then
    echo "$NEW" >> "$HISTORY_FILE"
    echo ""
    echo "🎉 更新完成！当前版本：$(git -C "$APP_DIR" rev-parse --short HEAD)"
    docker compose ps
else
    echo ""
    echo "❌ 部署失败！上一个版本仍在运行，可执行 ./rollback.sh 回滚。"
    exit 1
fi