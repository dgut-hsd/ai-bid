-- 把 Rust 词级/段落级高亮定位持久化到 audit_issue，
-- 使 Rust 引擎重启后 GET /result 的 DB 回退路径也能返回 block_ids / highlight_rects。
-- （此前 toIssue 落库未存、getResult DB 回退也未回填，导致重建 docker 后前端
--   拿不到 bbox，只能退回 pdf.js 文本层收敛，高亮肉眼观感差。）
ALTER TABLE `audit_issue`
    ADD COLUMN `block_ids` json DEFAULT NULL COMMENT '关联的原始 block_id 列表（bbox 段落级高亮）' AFTER `context`,
    ADD COLUMN `highlight_rects` json DEFAULT NULL COMMENT '词级精确高亮矩形（source_quote 命中词的逐行 union bbox）' AFTER `block_ids`;