#!/usr/bin/env bash
# F6 护栏单元测试：用 stub mysql 客户端验证 audit-guard.sh 的判定逻辑。
#
# 运行：bash deploy/tests/audit-guard-test.sh
set -euo pipefail

TEST_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
GUARD_LIB="$(cd "$TEST_DIR/.." && pwd)/lib/audit-guard.sh"

# ── stub 环境 ──────────────────────────────────────────────────────
STUB_BIN="$(mktemp -d)"
STUB_OUTPUT_FILE="$STUB_BIN/last-query.sql"
FAKE_PASSWORD="test-root-password"
ENV_FILE="$STUB_BIN/.env"
echo "MYSQL_ROOT_PASSWORD=$FAKE_PASSWORD" > "$ENV_FILE"

cat > "$STUB_BIN/mysql-stub" <<'STUB'
#!/usr/bin/env bash
# 记录收到的 SQL，并按 ACTIVE_COUNT 环境变量输出计数（模拟 mysql -N 单行输出）
echo "$@" | grep -oE "SELECT COUNT\(\*\).*" >> "$STUB_OUTPUT_FILE" 2>/dev/null || true
count="${ACTIVE_COUNT:-0}"
if [[ "$count" == "fail" ]]; then
  echo "ERROR 2002 (HY000): Can't connect to local MySQL server" >&2
  exit 1
fi
echo "$count"
STUB
chmod +x "$STUB_BIN/mysql-stub"

# ── 测试框架 ──────────────────────────────────────────────────────
PASS_COUNT=0
FAIL_COUNT=0

pass() { PASS_COUNT=$((PASS_COUNT+1)); echo "  ✅ $1"; }
fail() { FAIL_COUNT=$((FAIL_COUNT+1)); echo "  ❌ $1"; }

# 执行 ensure_idle，返回退出码
run_guard() {
  (
    export AIBID_GUARD_DEPLOY_DIR="$STUB_BIN"
    export AIBID_GUARD_ENV_FILE="$ENV_FILE"
    export AIBID_MYSQL_CLIENT="$STUB_BIN/mysql-stub"
    export ACTIVE_COUNT="${ACTIVE_COUNT:-0}"
    source "$GUARD_LIB"
    audit_guard_ensure_idle "$@"
  )
}

# ── 用例 ──────────────────────────────────────────────────────────

echo "== audit-guard 单元测试 =="

# 1. 无在途任务 → 放行
ACTIVE_COUNT=0
if run_guard > /dev/null 2>&1; then pass "无在途任务时放行"; else fail "无在途任务时应放行(exit=0)"; fi

# 2. 有在途任务 → 拦截
ACTIVE_COUNT=3
if run_guard > /dev/null 2>&1; then fail "有 3 个在途任务时应拦截"; else pass "有在途任务时拦截(exit!=0)"; fi

# 3. 有在途任务 + --force → 放行
ACTIVE_COUNT=3
if run_guard --force > /dev/null 2>&1; then pass "--force 时放行"; else fail "--force 时应放行"; fi

# 4. MySQL 查询失败 → 拦截（宁可误拦）
ACTIVE_COUNT=fail
if run_guard > /dev/null 2>&1; then fail "查询失败时应拦截"; else pass "查询失败时拦截(exit!=0)"; fi

# 5. MySQL 查询失败 + --force → 放行
ACTIVE_COUNT=fail
if run_guard --force > /dev/null 2>&1; then pass "查询失败 + --force 放行"; else fail "查询失败 + --force 时应放行"; fi

# 6. 无 .env → 拦截
rm -f "$ENV_FILE"
ACTIVE_COUNT=0
if run_guard > /dev/null 2>&1; then fail "缺少 .env 密码时应拦截"; else pass "缺少 .env 密码时拦截(exit!=0)"; fi

# 7. active_count 输出正确数字
echo "MYSQL_ROOT_PASSWORD=$FAKE_PASSWORD" > "$ENV_FILE"
ACTIVE_COUNT=5
COUNT_OUT=$( (
  export AIBID_GUARD_DEPLOY_DIR="$STUB_BIN"
  export AIBID_GUARD_ENV_FILE="$ENV_FILE"
  export AIBID_MYSQL_CLIENT="$STUB_BIN/mysql-stub"
  export ACTIVE_COUNT=5
  source "$GUARD_LIB"
  audit_guard_active_count
) )
if [[ "$COUNT_OUT" == "5" ]]; then pass "active_count=5 正确"; else fail "active_count 应为 5，实际=$COUNT_OUT"; fi

# 清理
rm -rf "$STUB_BIN"

echo ""
echo "== 结果：$PASS_COUNT 通过，$FAIL_COUNT 失败 =="
if [[ "$FAIL_COUNT" -gt 0 ]]; then
  exit 1
fi
exit 0
