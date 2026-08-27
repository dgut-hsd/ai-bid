#!/usr/bin/env bash
# 发布护栏：检测 ai-bid 是否存在进行中的审核任务。
#
# 背景：docker compose up -d --build 会重建 backend-rust 容器，而 Rust 引擎
# 将「进行中的审核」全部放在进程内存里。若发布时存在在途审核，重启会直接
# 制造孤儿任务（用户看到的卡死 30 分钟）。
#
# 用法（被 update.sh / rollback.sh source）：
#   source deploy/lib/audit-guard.sh
#   audit_guard_ensure_idle [--force]    # 有在途任务且无 --force → 退出 1
#
# 可测试性：MySQL 查询命令通过 AIBID_MYSQL_CLIENT 环境变量注入，
# 测试可以用 stub 脚本替换。

set -euo pipefail

# 依赖的部署目录（由调用方设置 DEPLOY_DIR）
AIBID_GUARD_DEPLOY_DIR="${AIBID_GUARD_DEPLOY_DIR:-${DEPLOY_DIR:-}}"

audit_guard_mysql_password() {
    local env_file="${AIBID_GUARD_ENV_FILE:-${AIBID_GUARD_DEPLOY_DIR}/.env}"
    if [[ -f "$env_file" ]]; then
        grep -E '^MYSQL_ROOT_PASSWORD=' "$env_file" | head -n1 | cut -d= -f2-
    else
        echo ""
    fi
}

# 输出进行中的审核任务数量（PENDING + PROCESSING）。
# 查询失败或输出异常时输出 -1（宁可误拦，不可漏拦：此时无法确认安全）。
audit_guard_active_count() {
    local password
    password="$(audit_guard_mysql_password)"
    local client="${AIBID_MYSQL_CLIENT:-docker exec aib-mysql mysql}"
    local raw
    if [[ -z "$password" ]]; then
        echo "-1" # 拿不到密码 → 无法确认，按不安全处理
        return
    fi
    raw="$($client -uroot -p"$password" smart_tender_system -N -e \
        "SELECT COUNT(*) FROM audit_task WHERE task_status IN (0,1);" 2>/dev/null)" \
        || raw=""
    # 归一化：取首行、去空白；必须是纯数字，否则视为查询失败
    local count
    count="$(printf '%s\n' "$raw" | head -n1 | tr -d '[:space:]')"
    if [[ ! "$count" =~ ^[0-9]+$ ]]; then
        echo "-1"
        return
    fi
    echo "$count"
}

# 存在在途审核时返回 1（除非 --force）；安全时返回 0。
audit_guard_ensure_idle() {
    local force=false
    if [[ "${1:-}" == "--force" ]]; then
        force=true
    fi

    local count
    count="$(audit_guard_active_count)"

    if [[ "$count" == "-1" ]]; then
        echo "⚠️  audit-guard：无法确认是否存在在途审核任务（MySQL 查询失败）。"
        if [[ "$force" == true ]]; then
            echo "   --force 已指定，继续执行。"
            return 0
        fi
        echo "   为避免重建 Rust 容器导致审核中断，已中止。确认安全后可加 --force 重试。"
        return 1
    fi

    if [[ "$count" -gt 0 ]]; then
        echo "⛔ audit-guard：检测到 $count 个进行中的审核任务（PENDING/PROCESSING）。"
        echo "   重建 backend-rust 会中断这些审核（Rust 审核状态在内存中）。"
        if [[ "$force" == true ]]; then
            echo "   --force 已指定，继续执行（在途审核将中断！）。"
            return 0
        fi
        echo "   已中止发布。等待任务完成后再执行，或明确知悉风险后加 --force。"
        return 1
    fi

    echo "✅ audit-guard：无进行中的审核任务，可以发布。"
    return 0
}
