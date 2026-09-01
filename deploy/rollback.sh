#!/usr/bin/env bash
# 回滚：把代码切回上一个「成功部署」的版本，重新构建并上线。
# 用法：./rollback.sh                   回退到上一个成功部署版本
#       ./rollback.sh <commit-hash>     回退到指定 commit
#       ./rollback.sh <commit-hash> --force  存在在途审核时强制回滚（会中断审核！）

set -euo pipefail

DEPLOY_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
APP_DIR="$(dirname "$DEPLOY_DIR")"
HISTORY_FILE="$DEPLOY_DIR/.deploy-history"
HOLD_FILE="$DEPLOY_DIR/.rollback-hold"

cd "$APP_DIR"

# 确定回退目标：显式 commit，或历史记录里的上一个成功版本
if [ $# -ge 1 ]; then
    TARGET="$1"
else
    if [ ! -f "$HISTORY_FILE" ] || [ "$(wc -l < "$HISTORY_FILE")" -lt 2 ]; then
        echo "❌ 没有可回滚的历史版本（至少需要 2 条部署记录）"
        echo "   历史记录文件：$HISTORY_FILE"
        echo "   也可手动指定：./rollback.sh <commit-hash>"
        exit 1
    fi
    TARGET="$(tail -n 2 "$HISTORY_FILE" | head -n 1)"
fi

# 校验目标存在
if ! git cat-file -e "$TARGET^{commit}" 2>/dev/null; then
    echo "❌ 目标版本不存在：$TARGET"
    exit 1
fi

# ── 发布护栏：回滚同样会重建 Rust，存在在途审核时需明确 --force ──
# 必须在 git checkout 之前拦截，避免中止后工作区已切换。
# rollback.sh 的参数 1 是 commit，--force 在参数 2
source "$DEPLOY_DIR/lib/audit-guard.sh"
if ! audit_guard_ensure_idle "${2:-}"; then
    echo ""
    echo "❌ 回滚已中止（存在进行中的审核任务）。"
    exit 1
fi

echo "==> 回滚到：$(git rev-parse --short "$TARGET")  ($(git log -1 --format=%s "$TARGET"))"
git checkout "$TARGET"

cd "$DEPLOY_DIR"

echo "==> 重新构建并部署回滚版本..."
docker compose up -d --build --remove-orphans

# 创建保持标记：避免自动更新任务立刻把你拉回 main
touch "$HOLD_FILE"

echo ""
echo "🔄 回滚完成！当前版本：$(git -C "$APP_DIR" rev-parse --short HEAD)"
echo "   已进入「回滚保持」状态，自动更新暂停。"
echo "   排查完问题后：删除 $HOLD_FILE 或执行 ./update.sh --force 恢复。"
docker compose ps