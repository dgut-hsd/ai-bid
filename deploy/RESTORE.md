# ai-bid 备份与恢复手册

## 一、备份概览

- 备份脚本：`deploy/backup.sh`
- 备份输出目录：`/srv/backups/ai-bid/`
- 定时任务（root crontab）：
  - 每天 02:30 → `./backup.sh daily`（数据库 + 上传文件）
  - 每周日 03:30 → `./backup.sh weekly`（数据库 + 上传 + Neo4j + Qdrant）
- 备份日志：`/srv/backups/ai-bid/backup.log`

### 备份内容与保留

| 目录 | 内容 | 频率 | 保留 |
|------|------|------|------|
| `mysql/` | 数据库 `smart_tender_system`（mysqldump 逻辑备份） | 每天 | 7 天 |
| `uploads/` | 上传的标书/文档（`ai-bid_app_data:/data/uploads`） | 每天 | 7 天 |
| `neo4j/` | Neo4j 图数据（`ai-bid_neo4j_data`） | 每周 | 4 份 |
| `qdrant/` | Qdrant 向量数据（`ai-bid_qdrant_data`） | 每周 | 4 份 |

> 注意：本机是 **snap docker**，容器写 bind mount 不会同步到宿主，所以卷备份用「辅助容器 tar + `docker cp`」导出，不要在宿主机上直接找卷目录。

### 手动执行备份

```bash
cd /srv/projects/ai-bid/deploy
sudo ./backup.sh daily     # 立即备份数据库+上传
sudo ./backup.sh weekly    # 立即全量(含 Neo4j/Qdrant)
```

---

## 二、恢复数据库

场景：数据库损坏/误删、需要回退到某天。

```bash
# 找到要恢复的备份
ls -lh /srv/backups/ai-bid/mysql/
# 例如: smart_tender_system_20260826_150401.sql.gz

# 方式一：覆盖当前库(先备份当前库到临时文件以防万一)
cd /srv/projects/ai-bid/deploy
gzip -dc /srv/backups/ai-bid/mysql/smart_tender_system_20260826_150401.sql.gz \
  | docker exec -i aib-mysql mysql -uroot -p"$MYSQL_ROOT_PASSWORD" smart_tender_system

# 方式二：若要整体重建库
docker exec -i aib-mysql mysql -uroot -p"$MYSQL_ROOT_PASSWORD" \
  -e "DROP DATABASE IF EXISTS smart_tender_system; CREATE DATABASE smart_tender_system CHARACTER SET utf8mb4;"
gzip -dc /srv/backups/ai-bid/mysql/*.sql.gz \
  | docker exec -i aib-mysql mysql -uroot -p"$MYSQL_ROOT_PASSWORD" smart_tender_system

# 验证
docker exec aib-mysql mysql -uroot -p"$MYSQL_ROOT_PASSWORD" smart_tender_system -e "SELECT COUNT(*) FROM sys_user;"
```

---

## 三、恢复上传文件

场景：`ai-bid_app_data` 卷里的上传标书/文档丢失。

```bash
# 找到备份
ls -lh /srv/backups/ai-bid/uploads/

# 解包到临时目录
mkdir -p /tmp/restore-uploads
tar xzf /srv/backups/ai-bid/uploads/uploads_20260826_150401.tgz -C /tmp/restore-uploads
# 解出内容在 /tmp/restore-uploads/uploads/

# 用辅助容器把文件拷回 ai-bid_app_data 卷的 /data/uploads
docker run -d --name rst-uploads -v ai-bid_app_data:/data alpine sleep 60
docker cp /tmp/restore-uploads/uploads/. rst-uploads:/data/uploads/
docker rm -f rst-uploads
```

---

## 四、恢复 Neo4j / Qdrant

场景：对应卷数据丢失/损坏。恢复后需重启对应容器。

### Neo4j

```bash
ls -lh /srv/backups/ai-bid/neo4j/
docker run -d --name rst-neo4j -v ai-bid_neo4j_data:/data alpine sleep 60
docker cp /srv/backups/ai-bid/neo4j/neo4j_20260826_150253.tgz rst-neo4j:/tmp/neo4j.tgz
docker exec rst-neo4j sh -c "rm -rf /data/* && tar xzf /tmp/neo4j.tgz -C /data"
docker rm -f rst-neo4j
docker restart aib-neo4j
```

### Qdrant

```bash
ls -lh /srv/backups/ai-bid/qdrant/
docker run -d --name rst-qdrant -v ai-bid_qdrant_data:/qdrant alpine sleep 60
docker cp /srv/backups/ai-bid/qdrant/qdrant_20260826_150253.tgz rst-qdrant:/tmp/qdrant.tgz
docker exec rst-qdrant sh -c "rm -rf /qdrant/* && tar xzf /tmp/qdrant.tgz -C /qdrant"
docker rm -f rst-qdrant
docker restart aib-qdrant
```

---

## 五、应急：项目目录误删 / 卷被删

### 1) 项目目录 `/srv/projects/ai-bid` 被删

数据都在 Docker 命名卷里，**不随项目目录删除而丢失**。重建方式：
```bash
# 重新 clone 仓库
git clone <repo-url> /srv/projects/ai-bid
# 从 deploy 恢复编排
cd /srv/projects/ai-bid/deploy && docker compose up -d
# 命名卷数据仍在, 容器会重新挂载
```

### 2) 命名卷被删（`docker volume rm ai-bid_*`）

**最严重**，卷数据无备份则无法恢复。必须从备份重建卷：
```bash
# 先重建卷(compose 会自动建), 再用上面第三/四节方法从备份灌回数据
```

> **核心原则**：备份脚本只保证「有备份可恢复」。请定期检查 `backup.log` 确认备份成功，并建议把 `/srv/backups` 同步到异地/对象存储以防磁盘故障。
