# Cypher 脚本（Neo4j 稳定性指标测试专用）

本文件汇总稳定性指标测试全流程用到的所有 Cypher 查询语句，按测试模块分类，可直接在 Neo4j 浏览器中执行。

------

## 一、节点与关系计数（幂等性测试前后校验）

用于幂等性测试前、后统计节点总数，验证重复写入无新增数据。

### 1. 无告警合并版（推荐）

使用`UNION ALL`拼接独立统计，规避笛卡尔积性能告警，一次执行返回全部结果。

cypher

```
MATCH (n:Law) RETURN 'law_count' AS 指标项, count(n) AS 数值
UNION ALL
MATCH (n:Article) RETURN 'article_count' AS 指标项, count(n) AS 数值
UNION ALL
MATCH (n:Risk) RETURN 'risk_count' AS 指标项, count(n) AS 数值
UNION ALL
MATCH ()-[r]->() RETURN 'relation_count' AS 指标项, count(r) AS 数值
```

### 2. 分条独立版

适合单独核对某一类节点数量，按需执行。

cypher

```
// 统计Law节点数
MATCH (n:Law) RETURN count(n) AS law_count;

// 统计Article节点数
MATCH (n:Article) RETURN count(n) AS article_count;

// 统计Risk节点数
MATCH (n:Risk) RETURN count(n) AS risk_count;

// 统计关系总数
MATCH ()-[r]->() RETURN count(r) AS relation_count;
```

------

## 二、round-trip 一致性批量查询

用于回查已入库节点属性，与原始 findings 文件做逐字段一致性比对。

### 1. Risk 节点批量查询

支持按`risk_id`或`candidate_ids`数组内的编号查询，返回核心校验字段。

cypher

```
MATCH (r:Risk)
WHERE r.risk_id IN ['risk_7739ef42', 'risk_3b9174d9']
   OR 'R_011' IN r.candidate_ids
   OR 'R_015' IN r.candidate_ids
   OR 'R_008' IN r.candidate_ids
   OR 'R_012' IN r.candidate_ids
   OR 'R_014' IN r.candidate_ids
RETURN r.risk_id, r.name, r.severity, r.candidate_ids
```

### 2. Law 节点批量查询

按法条名称模糊匹配，返回核心校验字段。

cypher

```
MATCH (l:Law)
WHERE l.name CONTAINS '政府采购货物和服务招标投标管理办法'
   OR l.name CONTAINS '政府采购法实施条例'
   OR l.name CONTAINS '政府采购法'
RETURN l.law_id, l.name, l.level
```

### 3. Article 节点批量查询

按条款号精确匹配，库内条款号格式为「第 + 阿拉伯数字 + 条」（如第 20 条）。

cypher

```
MATCH (a:Article)
WHERE a.article_no IN ['第20条', '第22条', '第26条']
RETURN a.article_id, a.article_no, a.law_id
```

------

## 三、属性完整率统计

统计三类节点核心必填字段的非空占比，验证入库数据字段完整性。

### 1. Law 节点完整率

cypher

```
MATCH (l:Law)
WITH count(l) AS total,
     count(CASE WHEN l.name IS NOT NULL AND l.level IS NOT NULL THEN 1 END) AS valid
RETURN total, valid, round(valid * 100.0 / total, 2) AS complete_rate;
```

### 2. Article 节点完整率

cypher

```
MATCH (a:Article)
WITH count(a) AS total,
     count(CASE WHEN a.article_no IS NOT NULL AND a.law_id IS NOT NULL THEN 1 END) AS valid
RETURN total, valid, round(valid * 100.0 / total, 2) AS complete_rate;
```

### 3. Risk 节点完整率

cypher

```
MATCH (r:Risk)
WITH count(r) AS total,
     count(CASE WHEN r.risk_id IS NOT NULL AND r.name IS NOT NULL AND r.severity IS NOT NULL THEN 1 END) AS valid
RETURN total, valid, round(valid * 100.0 / total, 2) AS complete_rate;
```

------

## 四、辅助查询语句

### 1. 查看单节点全量属性（字段核验用）

用于确认库内节点的真实字段名，避免属性名不匹配导致查询无结果。

cypher

```
// 查看1个Risk节点的全部属性
MATCH (r:Risk) RETURN r LIMIT 1;

// 查看1个Law节点的全部属性
MATCH (l:Law) RETURN l LIMIT 1;

// 查看1个Article节点的全部属性
MATCH (a:Article) RETURN a LIMIT 1;
```

### 2. 关联图谱查询（验收截图用）

查询风险→条款→法条的三级关联结构，用于生成 Neo4j 图谱截图，验证关联关系正确。

```
MATCH path=(r:Risk)-[:REFERENCES]->(a:Article)-[:BELONGS_TO]->(l:Law)
RETURN path
LIMIT 10
```