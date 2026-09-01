#!/usr/bin/env bash
# =============================================================
# ai-bid 数据备份脚本 (snap docker 兼容版)
# 用法: ./backup.sh daily    # 每天: 数据库 + 上传文件
#       ./backup.sh weekly   # 每周: 数据库 + 上传 + Neo4j + Qdrant
# 输出: /srv/backups/ai-bid/{mysql,uploads,neo4j,qdrant}
#
# 注意: 本机是 snap docker, 容器写 bind mount 不会同步到宿主,
#       因此卷备份用「辅助容器 tar + docker cp 导出」的方式。
# =============================================================
set -euo pipefail

MODE="${1:-daily}"
ROOT="${BACKUP_ROOT:-/srv/backups}/ai-bid"
MYSQL_CONTAINER="${MYSQL_CONTAINER:-aib-mysql}"
DB_NAME="${DB_NAME:-smart_tender_system}"
KEEP_DAILY="${KEEP_DAILY:-7}"      # 保留天数(数据库/上传)
KEEP_WEEKLY="${KEEP_WEEKLY:-4}"    # 保留份数(Neo4j/Qdrant)
ALPINE="${ALPINE_IMAGE:-alpine}"

[ "$MODE" = "daily" ] || [ "$MODE" = "weekly" ] || { echo "用法: $0 daily|weekly" >&2; exit 1; }

# 数据库密码从 aib-mysql 容器 env 读取(避免硬编码/命令行明文)
MYSQL_PWD="$(docker exec "$MYSQL_CONTAINER" sh -c 'echo -n "$MYSQL_ROOT_PASSWORD"')"
STAMP="$(date +%Y%m%d_%H%M%S)"
mkdir -p "$ROOT"/{mysql,uploads,neo4j,qdrant}

echo ">>> [$MODE] 备份 ai-bid 数据 → $ROOT (stamp=$STAMP)"

# 辅助容器 tar + docker cp 导出卷
# $1=卷名  $2=卷内挂载点  $3=导出子目录(相对挂载点)  $4=输出文件名  $5=输出目录
export_volume() {
  local vol="$1" mountpt="$2" src="$3" outname="$4" outdir="$5"
  local helper="bk_$(basename "$vol")_$$"
  docker run -d --name "$helper" -v "$vol":"$mountpt" "$ALPINE" sleep 60 >/dev/null
  # 归档写到容器 /tmp(不污染数据卷), 再用 docker cp 导出到宿主
  if docker exec "$helper" sh -c "tar czf /tmp/_bk.tgz -C '$mountpt' '$src'"; then
    docker cp "$helper:/tmp/_bk.tgz" "$outdir/$outname"
    echo "   OK: $(du -h "$outdir/$outname" | cut -f1)"
  else
    echo "   [ERROR] tar 失败"
  fi
  docker rm -f "$helper" >/dev/null
}

# ── 1. 数据库 (每天) ─────────────────────────────────────────
echo ">>> 数据库 $DB_NAME"
docker exec -e MYSQL_PWD="$MYSQL_PWD" "$MYSQL_CONTAINER" \
  mysqldump -uroot --single-transaction --routines --triggers --events "$DB_NAME" \
  | gzip > "$ROOT/mysql/${DB_NAME}_${STAMP}.sql.gz"
gzip -t "$ROOT/mysql/${DB_NAME}_${STAMP}.sql.gz"   # 校验
echo "   OK: $(du -h "$ROOT/mysql/${DB_NAME}_${STAMP}.sql.gz" | cut -f1)"
find "$ROOT/mysql" -name "${DB_NAME}_*.sql.gz" -mtime +"$KEEP_DAILY" -delete

# ── 2. 上传文件 (每天) ───────────────────────────────────────
echo ">>> 上传文件 (ai-bid_app_data:/data/uploads)"
export_volume ai-bid_app_data /data uploads "uploads_${STAMP}.tgz" "$ROOT/uploads"
find "$ROOT/uploads" -name 'uploads_*.tgz' -mtime +"$KEEP_DAILY" -delete

# ── 3. Neo4j + Qdrant (每周) ─────────────────────────────────
if [ "$MODE" = "weekly" ]; then
  echo ">>> Neo4j (ai-bid_neo4j_data)"
  export_volume ai-bid_neo4j_data /data . "neo4j_${STAMP}.tgz" "$ROOT/neo4j"
  echo ">>> Qdrant (ai-bid_qdrant_data)"
  export_volume ai-bid_qdrant_data /qdrant . "qdrant_${STAMP}.tgz" "$ROOT/qdrant"

  # 清理: 保留最近 N 份
  find "$ROOT/neo4j" -name 'neo4j_*.tgz' -mtime +$((KEEP_WEEKLY*7)) -delete
  find "$ROOT/qdrant" -name 'qdrant_*.tgz' -mtime +$((KEEP_WEEKLY*7)) -delete
fi

echo ">>> [$MODE] 备份完成"
