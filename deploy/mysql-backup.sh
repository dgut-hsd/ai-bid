#!/usr/bin/env bash
#
# MySQL 逻辑备份脚本（配合 Docker 使用）
#
# ── 用法 ────────────────────────────────────────────────────────
# 1) 环境变量（均可用同名环境变量覆盖）：
#      MYSQL_CONTAINER       容器名              默认 aib-mysql
#      MYSQL_DATABASE        数据库名            默认 smart_tender_system
#      MYSQL_USER            备份账号            默认 root
#      MYSQL_ROOT_PASSWORD   root 密码（必填，脚本拒绝命令行明文传密码）
#      BACKUP_DIR            备份目录            默认 /srv/apps/ai-bid/backup/mysql
#      KEEP_DAYS             保留天数            默认 7
#
# 2) 手动执行：
#      MYSQL_ROOT_PASSWORD='你的密码' ./deploy/mysql-backup.sh
#
# 3) 定时任务（crontab -e），每天 02:30 执行、保留 7 天：
#      30 2 * * * cd /srv/apps/ai-bid && MYSQL_ROOT_PASSWORD='你的密码' ./deploy/mysql-backup.sh >> /srv/apps/ai-bid/backup/backup.log 2>&1
#    （生产环境建议把密码放进 root 只读文件，例如 echo 'MYSQL_ROOT_PASSWORD=xxx' > /srv/apps/ai-bid/.backup-env
#      然后 crontab 写成：cd /srv/apps/ai-bid && set -a && . ./.backup-env && set +a && ./deploy/mysql-backup.sh）
#
# ── 恢复（应急）────────────────────────────────────────────────
#     gzip -dc 备份文件.sql.gz | docker exec -i aib-mysql mysql -uroot -p smart_tender_system
#
set -euo pipefail

CONTAINER="${MYSQL_CONTAINER:-aib-mysql}"
DB_NAME="${MYSQL_DATABASE:-smart_tender_system}"
DB_USER="${MYSQL_USER:-root}"
DB_PASSWORD="${MYSQL_ROOT_PASSWORD:-}"
BACKUP_DIR="${BACKUP_DIR:-/srv/apps/ai-bid/backup/mysql}"
KEEP_DAYS="${KEEP_DAYS:-7}"

if [ -z "${DB_PASSWORD}" ]; then
  echo "[ERROR] 未设置 MYSQL_ROOT_PASSWORD，无法备份（密码请通过环境变量传入，不要写在命令行）" >&2
  exit 1
fi

mkdir -p "${BACKUP_DIR}"

STAMP="$(date +%Y%m%d_%H%M%S)"
OUT="${BACKUP_DIR}/${DB_NAME}_${STAMP}.sql.gz"

# --single-transaction：InnoDB 一致性快照，备份期间不锁表
# --routines --triggers --events：连存储过程/触发器/事件一起备份
# MYSQL_PWD 通过 docker exec -e 注入容器，避免密码出现在进程命令行(ps)里
docker exec -e MYSQL_PWD="${DB_PASSWORD}" "${CONTAINER}" \
  mysqldump -u"${DB_USER}" \
    --single-transaction --routines --triggers --events \
    "${DB_NAME}" | gzip > "${OUT}"

# 校验 gzip 完整性，损坏立即失败退出
gzip -t "${OUT}"

# 清理 N 天前的旧备份
find "${BACKUP_DIR}" -name "${DB_NAME}_*.sql.gz" -mtime +"${KEEP_DAYS}" -delete

echo "[OK] 备份完成: ${OUT} ($(du -h "${OUT}" | cut -f1))"