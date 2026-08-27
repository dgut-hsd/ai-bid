#!/usr/bin/env bash
# 自动更新：拉取部署分支最新代码 → 重新构建 → 滚动更新容器 → 冒烟测试。
# 用法：./update.sh          正常更新
#       ./update.sh --force  跳过「在途审核」护栏（不清除未提交改动）
#
# 部署分支用环境变量 DEPLOY_BRANCH 覆盖（默认 dev，与本机实际运行分支一致）。
# 注意：本机生产跟随 dev；待 dev → main 合并同步后，可 export DEPLOY_BRANCH=main 切换。

set -euo pipefail

DEPLOY_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
APP_DIR="$(dirname "$DEPLOY_DIR")"
HISTORY_FILE="$DEPLOY_DIR/.deploy-history"
HOLD_FILE="$DEPLOY_DIR/.rollback-hold"
DEPLOY_BRANCH="${DEPLOY_BRANCH:-dev}"
SMOKE_URL="${SMOKE_URL:-http://127.0.0.1/aibid/api/health}"
SMOKE_RETRIES="${SMOKE_RETRIES:-40}"   # 每次 3s，共约 120s 等待后端就绪

FORCE=false
for a in "$@"; do
  [ "$a" = "--force" ] && FORCE=true
done

# --force：仅清除回滚保持标记（不清除未提交改动，那由下面的工作区守卫负责）
if [ "$FORCE" = true ]; then
    rm -f "$HOLD_FILE"
    echo "==> 强制更新模式（已清除回滚保持标记）"
fi

# 处于回滚保持状态时，避免自动更新把回滚版本覆盖掉
if [ -f "$HOLD_FILE" ]; then
    echo "⏸️  当前处于「回滚保持」状态，暂不自动更新。"
    echo "   排查完问题后：删除 $HOLD_FILE 或执行 ./update.sh --force 恢复追 $DEPLOY_BRANCH。"
    exit 0
fi

cd "$APP_DIR"

# 工作区守卫：存在未提交改动时中止，避免 checkout 失败或把改动带错分支
if [ -n "$(git status --porcelain)" ]; then
    echo "❌ 工作区存在未提交改动，发布会丢失/误带这些改动。"
    echo "   请先 git add/commit（或 stash）后再发布。当前改动："
    git status --short
    exit 1
fi

echo "==> 切到部署分支 $DEPLOY_BRANCH 并检查更新..."
git fetch origin "$DEPLOY_BRANCH"
git checkout "$DEPLOY_BRANCH"

LOCAL=$(git rev-parse HEAD)
REMOTE=$(git rev-parse "origin/$DEPLOY_BRANCH")

if ! git merge-base --is-ancestor "$LOCAL" "$REMOTE"; then
    echo "❌ 本地 $DEPLOY_BRANCH 与 origin/$DEPLOY_BRANCH 已分叉（或本地领先未推送）。"
    echo "   为避免静默回退，已中止。请先 push 本地提交、或人工合并后再重试。"
    exit 1
fi

if [ "$LOCAL" = "$REMOTE" ]; then
    echo "✅ 已是最新版本 $(git rev-parse --short HEAD)，无需更新。"
    echo "   （如仅需重建/重启容器：docker compose up -d --build）"
    exit 0
fi

echo "==> 发现新版本：$(git rev-parse --short "$LOCAL") → $(git rev-parse --short "$REMOTE")"
git pull --ff-only origin "$DEPLOY_BRANCH"
NEW=$(git rev-parse HEAD)

cd "$DEPLOY_DIR"

# ── 发布护栏：存在在途审核时重建 Rust 会制造孤儿任务 ──────────────
# shellcheck source=lib/audit-guard.sh
source "$DEPLOY_DIR/lib/audit-guard.sh"
FORCE_FLAG=""
[ "$FORCE" = true ] && FORCE_FLAG="--force"
if ! audit_guard_ensure_idle "$FORCE_FLAG"; then
    echo ""
    echo "❌ 发布已中止（存在进行中的审核任务）。"
    exit 1
fi

echo "==> 重新构建并滚动更新（数据卷不动）..."
if ! docker compose up -d --build --remove-orphans; then
    echo ""
    echo "❌ 部署失败！上一个版本仍在运行，可执行 ./rollback.sh 回滚。"
    exit 1
fi

echo "==> 冒烟测试: $SMOKE_URL"
ok=0
for _ in $(seq 1 "$SMOKE_RETRIES"); do
    code="$(curl -s -o /dev/null -w '%{http_code}' "$SMOKE_URL" 2>/dev/null || true)"
    if [ "$code" = "200" ]; then ok=1; break; fi
    sleep 3
done
if [ "$ok" != "1" ]; then
    echo "❌ 冒烟测试未通过（$SMOKE_URL 未返回 200）。请查容器状态/日志，必要时 ./rollback.sh 回滚。"
    docker compose ps
    exit 1
fi

echo "$NEW" >> "$HISTORY_FILE"
echo ""
echo "🎉 更新完成！当前版本：$(git -C "$APP_DIR" rev-parse --short HEAD)"
docker compose ps