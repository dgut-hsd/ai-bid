use super::types::*;
use std::collections::HashSet;
use regex::Regex;
use sha2::{Digest, Sha256};

// --------------------------
// 单元1：中文数字转阿拉伯数字（辅助工具）
// --------------------------
/// 把中文数字（零~九百九十九）转成 u32
fn chinese_num_to_u32(s: &str) -> Option<u32> {
    let map = |c: char| -> Option<u32> {
        match c {
            '零' => Some(0),
            '一' | '壹' => Some(1),
            '二' | '贰' | '两' => Some(2),
            '三' | '叁' => Some(3),
            '四' | '肆' => Some(4),
            '五' | '伍' => Some(5),
            '六' | '陆' => Some(6),
            '七' | '柒' => Some(7),
            '八' | '捌' => Some(8),
            '九' | '玖' => Some(9),
            _ => None,
        }
    };

    let mut result: u32 = 0;
    let mut temp: u32 = 0;
    let chars: Vec<char> = s.chars().collect();

    for &c in &chars {
        match c {
            '百' | '佰' => {
                result += temp * 100;
                temp = 0;
            }
            '十' | '拾' => {
                if temp == 0 {
                    temp = 1; // "十"开头 = 10
                }
                result += temp * 10;
                temp = 0;
            }
            _ => {
                let n = map(c)?;
                temp = temp * 10 + n;
            }
        }
    }
    result += temp;
    Some(result)
}

// --------------------------
// 单元2：全角转半角 + 条款号归一化
// --------------------------
/// 全角字符转半角（数字、字母、空格、点、横线），其他字符原样保留。
fn to_half_width(s: &str) -> String {
    s.chars()
        .map(|c| match c {
            '０'..='９' => (c as u32 - 0xFF10 + 0x30) as u8 as char,
            'ａ'..='ｚ' => (c as u32 - 0xFF41 + 0x61) as u8 as char,
            'Ａ'..='Ｚ' => (c as u32 - 0xFF21 + 0x41) as u8 as char,
            '　' => ' ',
            '．' => '.',
            '－' => '-',
            _ => c,
        })
        .collect()
}

/// 条款号归一化："第二十条" → "第20条"，"第22条"保持不变，
/// "第３．２条" → "第3.2条"，"第三条之二" → "第3条之2"
pub fn normalize_article_number(raw: &str) -> Option<String> {
    let s = to_half_width(raw);
    let re = Regex::new(r"第\s*([零一二三四五六七八九十百千0-9.]+?)\s*条(之\s*([零一二三四五六七八九十0-9]+))?")
        .ok()?;
    let caps = re.captures(&s)?;
    let num_str = caps.get(1)?.as_str();

    // 纯阿拉伯数字（可含小数点）直接保留，否则中文数字转阿拉伯
    let main = if num_str.chars().all(|c| c.is_ascii_digit() || c == '.') {
        num_str.to_string()
    } else {
        chinese_num_to_u32(num_str)?.to_string()
    };

    let mut out = format!("第{}条", main);
    if let Some(suffix) = caps.get(3) {
        let sfx = suffix.as_str();
        let n = if sfx.chars().all(|c| c.is_ascii_digit()) {
            sfx.to_string()
        } else {
            chinese_num_to_u32(sfx)?.to_string()
        };
        out.push_str(&format!("之{}", n));
    }
    Some(out)
}

// --------------------------
// 单元3：法律依据字符串解析
// --------------------------
/// 去掉 Markdown 链接外壳："[文本](url)" → "文本"；无链接则原样返回。
fn strip_markdown(text: &str) -> String {
    match Regex::new(r"\[([^\]]+)\]\([^)]*\)") {
        Ok(re) => re.replace(text, "$1").to_string(),
        Err(_) => text.to_string(),
    }
}

/// 法律名规范化：同一部法律的不同书写格式收敛为同一名字。
/// 处理：条款号粘连（"X法第20条"→"X法"）、括号版本/文号（"…(国务院令第658号)"→"…"）、
/// "中华人民共和国"前缀（"中华人民共和国X法"→"X法"）、发文机关前缀（"国务院办公厅关于…"→"关于…"）。
fn normalize_law_name(name: &str) -> String {
    let mut s = name.trim().to_string();

    // 剔除条款号及其后的内容
    if let Ok(re) = Regex::new(r"第[零一二三四五六七八九十百千0-9.]+条.*") {
        s = re.replace(&s, "").to_string();
    }

    // 剔除括号（版本/文号说明）
    if let Ok(re) = Regex::new(r"[（(][^）)]*[）)]") {
        s = re.replace(&s, "").to_string();
    }

    // 合并国名前缀变体："中华人民共和国政府采购法" → "政府采购法"
    s = s.replace("中华人民共和国", "");

    // 合并发文机关前缀变体："国务院办公厅关于…" → "关于…"
    if let Some(idx) = s.find("关于") {
        s = s[idx..].to_string();
    }

    s.trim().trim_matches('《').trim_matches('》').trim().to_string()
}

/// 从法律依据字符串拆出规范化法律名和条款号。
/// 兼容 LLM 输出的多种格式：
///   "《政府采购法实施条例》第二十条"
///   "[中华人民共和国政府采购法实施条例第二十条](https://…)"
///   "政府采购法实施条例第二十条"
///   "中华人民共和国政府采购法实施条例(国务院令第658号)"
pub fn parse_law_basis(text: &str) -> (String, Option<String>) {
    // 先转半角、剥掉 Markdown 链接外壳
    let raw = strip_markdown(&to_half_width(text));

    // 匹配书名号里的法律名，无书名号则整段作为候选名
    let law_re = Regex::new(r"《([^》]+)》").unwrap();
    let law_name = law_re
        .captures(&raw)
        .and_then(|c| c.get(1))
        .map(|m| m.as_str().to_string())
        .unwrap_or_else(|| raw.clone());

    // 匹配条款号（含之N/小数）
    let article_re = Regex::new(r"第[零一二三四五六七八九十百千0-9.]+条(之[零一二三四五六七八九十0-9]+)?")
        .unwrap();
    let article_no = article_re
        .find(&raw)
        .map(|m| normalize_article_number(m.as_str()))
        .flatten();

    // 法律名规范化
    let law_name = normalize_law_name(&law_name);

    (law_name, article_no)
}

// --------------------------
// 单元3.5：法律元数据解析（效力层级 / 发文机关 / 文号 / 年份）
// --------------------------
/// 文号前缀 → 发文机关。
fn issuing_body_from_doc(prefix: &str) -> String {
    match prefix {
        "国发" | "国办发" => "国务院办公厅".to_string(),
        "财库" | "财综" | "财预" | "财采" | "财办" => "财政部".to_string(),
        "发改" | "发改价格" | "发改办" => "国家发展改革委".to_string(),
        _ => prefix.to_string(),
    }
}

/// 从原始法条引用解析法律元数据。
///
/// 优先解析文号（"财政部令第94号" / "国办发〔2016〕49号" / "财库〔2019〕38号"），
/// 无文号时按法律名后缀推断效力层级。
pub fn parse_law_meta(raw: &str, law_name: &str) -> Option<LawMeta> {
    let text = to_half_width(raw);

    // 文号 1：XX令第N号（财政部令第94号 / 国务院令第658号）
    if let Some(caps) = Regex::new(r"([\u4e00-\u9fa5]{2,10}?)令第(\d+)号")
        .ok()
        .and_then(|re| re.captures(&text))
    {
        let body = caps.get(1).map(|m| m.as_str()).unwrap_or("");
        let num = caps.get(2).map(|m| m.as_str()).unwrap_or("");
        let doc = format!("{}令第{}号", body, num);
        let level = if body.contains("国务院") { "行政法规" } else { "部门规章" };
        return Some(LawMeta {
            level: level.into(),
            issuing_body: body.into(),
            doc_number: doc,
            year: None,
            ..Default::default()
        });
    }

    // 文号 2：XX〔YYYY〕N号（国办发〔2016〕49号 / 财库〔2019〕38号 / 发改价格〔2018〕51号）
    if let Some(caps) = Regex::new(r"([\u4e00-\u9fa5]{1,12}?)〔(\d{4})〕(\d+)号")
        .ok()
        .and_then(|re| re.captures(&text))
    {
        let prefix = caps.get(1).map(|m| m.as_str()).unwrap_or("");
        let year = caps.get(2).map(|m| m.as_str().to_string());
        let num = caps.get(3).map(|m| m.as_str()).unwrap_or("");
        let doc = format!("{}〔{}〕{}号", prefix, year.as_deref().unwrap_or(""), num);
        return Some(LawMeta {
            level: "规范性文件".into(),
            issuing_body: issuing_body_from_doc(prefix),
            doc_number: doc,
            year,
            ..Default::default()
        });
    }

    // 无文号：按名称后缀推断效力层级
    // 注意判断顺序：先排除"办法/规定/规则/指南/通知/意见"，再判断"法"，避免"办法"误判为"法律"。
    let level = if law_name.ends_with("条例") {
        "行政法规"
    } else if law_name.ends_with("办法") || law_name.ends_with("规定") || law_name.ends_with("规则") {
        "部门规章"
    } else if law_name.ends_with("指南") || law_name.ends_with("通知") || law_name.ends_with("意见") {
        "规范性文件"
    } else if law_name.ends_with("法") || law_name.ends_with("典") {
        "法律"
    } else {
        "未分类"
    };
    Some(LawMeta {
        level: level.into(),
        issuing_body: String::new(),
        doc_number: String::new(),
        year: None,
        ..Default::default()
    })
}

// --------------------------
// 单元4：确定性 ID 生成
// --------------------------
/// 生成 risk_id：SHA256("risk:" + risk_type) 前8位 + risk_ 前缀
pub fn gen_risk_id(risk_type: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(b"risk:");
    hasher.update(risk_type.as_bytes());
    let hash = hex::encode(hasher.finalize());
    format!("risk_{}", &hash[..8])
}

/// 生成 law_id：SHA256(法律名) 前8位 + law_ 前缀
pub fn gen_law_id(law_name: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(law_name.as_bytes());
    let hash = hex::encode(hasher.finalize());
    format!("law_{}", &hash[..8])
}

/// 生成 article_id：SHA256(law_id + 归一化条款号) 前8位 + art_ 前缀
pub fn gen_article_id(law_id: &str, article_no: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(law_id.as_bytes());
    hasher.update(article_no.as_bytes());
    let hash = hex::encode(hasher.finalize());
    format!("art_{}", &hash[..8])
}

/// 生成 case_id：SHA256(case_ref 原文) 前8位 + case_ 前缀
pub fn gen_case_id(case_ref: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(b"case:");
    hasher.update(case_ref.as_bytes());
    let hash = hex::encode(hasher.finalize());
    format!("case_{}", &hash[..8])
}

/// 生成 rule_id：SHA256(禁止内容) 前8位 + rule_ 前缀
pub fn gen_rule_id(content: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(b"rule:");
    hasher.update(content.as_bytes());
    let hash = hex::encode(hasher.finalize());
    format!("rule_{}", &hash[..8])
}

/// 生成 experience_id：SHA256(risk_id + ":" + candidate_id + ":" + batch_key) 前8位 + exp_ 前缀。
///
/// 幂等语义：同一文档（batch_key）同风险类型同候选重投 → 同 ID → MERGE 幂等；
/// 跨文档去重：不同 batch_key 必产生不同 ID，避免"同风险类型 + 同候选序号"跨审核撞 ID、
/// 导致第二次审核的处置经验被 MERGE 静默丢弃。
pub fn gen_experience_id(risk_id: &str, candidate_id: &str, batch_key: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(b"exp:");
    hasher.update(risk_id.as_bytes());
    hasher.update(b":");
    hasher.update(candidate_id.as_bytes());
    hasher.update(b":");
    hasher.update(batch_key.as_bytes());
    let hash = hex::encode(hasher.finalize());
    format!("exp_{}", &hash[..8])
}

/// 从 case_ref 推断发布机关：匹配开头的机关名或取整体。
fn case_issuing_body(case_ref: &str) -> String {
    let t = case_ref.trim();
    if t.starts_with("财政部") {
        "财政部".to_string()
    } else if t.starts_with("国务院") {
        "国务院".to_string()
    } else if t.starts_with("最高人民法院") {
        "最高人民法院".to_string()
    } else if t.starts_with("人民法院") {
        "人民法院".to_string()
    } else {
        "未分类".to_string()
    }
}

/// 从 case_ref 推断年度：匹配 4 位阿拉伯年份。
fn case_year(case_ref: &str) -> Option<i64> {
    Regex::new(r"(19|20)\d{2}")
        .ok()
        .and_then(|re| re.captures(case_ref))
        .and_then(|c| c.get(0))
        .and_then(|m| m.as_str().parse::<i64>().ok())
}

/// 从 case_ref 推断案例类型。
fn case_type(case_ref: &str) -> String {
    let s = case_ref;
    for (kw, ty) in [
        ("投诉处理决定", "投诉处理决定"),
        ("行政处罚", "行政处罚决定"),
        ("判决", "司法判决"),
        ("裁决", "司法判决"),
        ("复议", "行政复议决定"),
        ("处理决定", "投诉处理决定"),
    ] {
        if s.contains(kw) {
            return ty.to_string();
        }
    }
    "未分类".to_string()
}

/// 从 risk_type 判断所属审查维度。
fn infer_dimension(risk_type: &str) -> (&'static str, &'static str, &'static str) {
    let t = risk_type;
    if t.contains("资格") || t.contains("条件") || t.contains("地域") || t.contains("注册") {
        PRESET_DIMENSIONS[0]
    } else if t.contains("程序") || t.contains("时间") || t.contains("公告") || t.contains("流程") {
        PRESET_DIMENSIONS[1]
    } else if t.contains("评审") || t.contains("评分") || t.contains("标准") || t.contains("加分") {
        PRESET_DIMENSIONS[2]
    } else if t.contains("技术") || t.contains("参数") || t.contains("品牌") || t.contains("规格") {
        PRESET_DIMENSIONS[3]
    } else if t.contains("合同") {
        PRESET_DIMENSIONS[4]
    } else if t.contains("需求") {
        PRESET_DIMENSIONS[5]
    } else {
        PRESET_DIMENSIONS[6]
    }
}

// --------------------------
// 单元5：主函数 - 实体拆分 + 查重
// --------------------------
pub fn extract_and_dedup(
    candidates: Vec<Candidate>,
    existing_law_ids: &HashSet<String>,
    batch_key: &str,
) -> Vec<EntityDecision> {
    candidates
        .into_iter()
        .map(|cand| {
            // 构造风险实体
            let risk = RiskEntity {
                id: gen_risk_id(&cand.risk_type),
                name: cand.risk_type.clone(),
                severity: cand.severity.clone(),
            };

            // 拆分所有法律依据 → Law + Article 实体
            let laws: Vec<LawArticleEntity> = cand
                .legal_basis
                .iter()
                .map(|basis| {
                    let (law_name, article_no) = parse_law_basis(basis);
                    let law_id = gen_law_id(&law_name);
                    let article_id = article_no
                        .as_ref()
                        .map(|no| gen_article_id(&law_id, no));
                    let mut meta = parse_law_meta(basis, &law_name);

                    // 时效状态默认 effective（文档 §9.4.4：effective | amended | repealed）
                    if let Some(m) = meta.as_mut() {
                        if m.status.is_empty() {
                            m.status = "effective".to_string();
                        }
                        // effective_date：当前从文号年份推断（外部数据源可覆盖）
                        if m.effective_date.is_empty() {
                            if let Some(ref y) = m.year {
                                m.effective_date = y.clone();
                            }
                        }
                    }
                    // short_name：去掉"中华人民共和国"等前缀后的常用简称
                    let short_name = normalize_law_name(&law_name);
                    // summary / full_text 不承载单次审核内容（reason/摘录），留空由外部条款库填充
                    LawArticleEntity {
                        law_id,
                        law_name,
                        short_name,
                        article_id,
                        article_no,
                        summary: String::new(),
                        meta,
                    }
                })
                .collect();

            // case_refs → 独立 Case 实体（设计文档 §9.4.4：Case 节点 + cited_in 关系）
            let cases: Vec<CaseEntity> = cand
                .case_refs
                .iter()
                .map(|cr| CaseEntity {
                    case_id: gen_case_id(cr),
                    title: cr.clone(),
                    issuing_body: case_issuing_body(cr),
                    year: case_year(cr),
                    case_type: case_type(cr),
                    // 案例结论摘要无数据源，留空（不承载单次审核的 reason/suggestion）
                    summary: String::new(),
                    full_text: String::new(),
                    keywords: {
                        let mut v = vec![cand.risk_type.clone()];
                        v.push(cand.severity.clone());
                        v
                    },
                })
                .collect();

            // risk_type → ProhibitionRule 实体（文档 §9.4.4：禁止模式目录，content 用稳定文案 → rule_id 稳定归并；
            // reason 原文留在 risk_experience，这里不做逐审核的临时文案）
            let content = format!("禁止{}类行为", cand.risk_type);
            let (dim_id, dim_name, dim_desc) = infer_dimension(&cand.risk_type);
            let rule = RuleEntity {
                rule_id: gen_rule_id(&content),
                content,
                source: String::new(),
                category: dim_name.to_string(),
                severity: if cand.severity == "high" {
                    "红线".to_string()
                } else {
                    "黄线".to_string()
                },
            };

            let dimension = Some(DimensionEntity {
                dimension_id: dim_id.to_string(),
                name: dim_name.to_string(),
                description: dim_desc.to_string(),
            });

            // 查重判断：只要有一个 law_id 不在库里，就标记为 New
            let has_new_law = laws.iter().any(|law| !existing_law_ids.contains(&law.law_id));
            let decision = if has_new_law {
                Decision::New
            } else {
                Decision::Exists
            };

            // 每次审核必产生一条处置经验（独立 RiskExperience 节点，不依赖 case_refs）：
            // 事实锚点 + 理由 + 建议。ID 基于 risk_id + candidate_id + batch_key 稳定幂等
            // （batch_key = 文档确定性标识，保证跨文档不撞 ID）。
            let risk_experience = RiskExperience {
                experience_id: gen_experience_id(&risk.id, &cand.candidate_id, batch_key),
                candidate_id: cand.candidate_id.clone(),
                source_quote: cand.source_quote.clone(),
                reason: cand.reason.clone(),
                suggestion: cand.suggestion.clone(),
                confidence: cand.confidence,
            };

            EntityDecision {
                candidate_id: cand.candidate_id,
                decision,
                risk,
                laws,
                cases,
                rules: vec![rule],
                dimension,
                risk_experience,
            }
        })
        .collect()
}

// --------------------------
// 单元测试
// --------------------------
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_chinese_num() {
        assert_eq!(chinese_num_to_u32("二十"), Some(20));
        assert_eq!(chinese_num_to_u32("二十二"), Some(22));
        assert_eq!(chinese_num_to_u32("十"), Some(10));
        assert_eq!(chinese_num_to_u32("五"), Some(5));
        assert_eq!(chinese_num_to_u32("一百二十三"), Some(123));
    }

    #[test]
    fn test_normalize_article() {
        assert_eq!(
            normalize_article_number("第二十条"),
            Some("第20条".to_string())
        );
        assert_eq!(
            normalize_article_number("第二十二条"),
            Some("第22条".to_string())
        );
        assert_eq!(
            normalize_article_number("第5条"),
            Some("第5条".to_string())
        );
        // 全角数字 + 小数点（文档任务 1 要求）
        assert_eq!(
            normalize_article_number("第３．２条"),
            Some("第3.2条".to_string())
        );
        // 之N 子条款（文档任务 1 要求）
        assert_eq!(
            normalize_article_number("第三条之二"),
            Some("第3条之2".to_string())
        );
    }

    #[test]
    fn test_parse_law_basis() {
        let text = "《政府采购法实施条例》第二十条";
        let (name, article) = parse_law_basis(text);
        assert_eq!(name, "政府采购法实施条例");
        assert_eq!(article, Some("第20条".to_string()));

        // 无条款号的情况
        let text2 = "《政府采购法》";
        let (name2, article2) = parse_law_basis(text2);
        assert_eq!(name2, "政府采购法");
        assert!(article2.is_none());
    }

    #[test]
    fn test_parse_dirty_formats_merge_to_same_id() {
        // 真实库里出现过的脏格式，都应收敛到同一个 law_id
        let dirty = [
            "《中华人民共和国政府采购法实施条例》第二十条",
            "[中华人民共和国政府采购法实施条例第二十条](https://xzfg.moj.gov.cn/front/law/detail?LawID=417)",
            "政府采购法实施条例第二十条",
            "政府采购法实施条例 第二十条",
            "中华人民共和国政府采购法实施条例(国务院令第658号)",
            "[《中华人民共和国政府采购法实施条例》](https://xzfg.moj.gov.cn/front/law/detail?LawID=417)",
        ];
        let ids: HashSet<String> = dirty
            .iter()
            .map(|t| {
                let (name, _) = parse_law_basis(t);
                gen_law_id(&name)
            })
            .collect();
        assert_eq!(ids.len(), 1, "不同格式的同一法律必须映射到同一 law_id");

        // 需求管理办法的脏格式同理
        let dirty2 = [
            "政府采购需求管理办法",
            "中华人民共和国政府采购需求管理办法",
            "[政府采购需求管理办法第九条](https://baike.baidu.com/item/政府采购需求管理办法/56971221)",
            "政府采购需求管理办法第九条",
        ];
        let ids2: HashSet<String> = dirty2
            .iter()
            .map(|t| {
                let (name, _) = parse_law_basis(t);
                gen_law_id(&name)
            })
            .collect();
        assert_eq!(ids2.len(), 1);

        // 国名/发文机关前缀变体
        let dirty3 = [
            "政府采购法",
            "中华人民共和国政府采购法",
            "国务院办公厅关于促进政府采购公平竞争优化营商环境的通知",
            "关于促进政府采购公平竞争优化营商环境的通知",
        ];
        let ids3: HashSet<String> = dirty3
            .iter()
            .map(|t| {
                let (name, _) = parse_law_basis(t);
                gen_law_id(&name)
            })
            .collect();
        assert_eq!(ids3.len(), 2); // 政府采购法一组，通知一组
    }

    #[test]
    fn test_parse_dirty_formats_article() {
        // Markdown 链接里的条款号仍被正确抽取
        let (name, article) =
            parse_law_basis("[政府采购需求管理办法第九条](https://baike.baidu.com/item/x)");
        assert_eq!(name, "政府采购需求管理办法");
        assert_eq!(article, Some("第9条".to_string()));

        // 无条款号时不产生 article
        let (_, article2) = parse_law_basis("[政府采购信息公告管理办法](https://x.com)");
        assert!(article2.is_none());
    }

    #[test]
    fn test_parse_law_meta_decree() {
        // 部门规章：财政部令第94号
        let m = parse_law_meta("《政府采购质疑和投诉办法》（财政部令第94号）", "政府采购质疑和投诉办法")
            .unwrap();
        assert_eq!(m.level, "部门规章");
        assert_eq!(m.issuing_body, "财政部");
        assert_eq!(m.doc_number, "财政部令第94号");
        assert!(m.year.is_none());

        // 行政法规：国务院令
        let m = parse_law_meta(
            "《中华人民共和国政府采购法实施条例》（国务院令第658号）",
            "政府采购法实施条例",
        )
        .unwrap();
        assert_eq!(m.level, "行政法规");
        assert_eq!(m.issuing_body, "国务院");
        assert_eq!(m.doc_number, "国务院令第658号");
    }

    #[test]
    fn test_parse_law_meta_doc_number() {
        // 规范性文件：财库〔2019〕38号
        let m = parse_law_meta(
            "财政部《关于促进政府采购公平竞争优化营商环境的通知》（财库〔2019〕38号）",
            "关于促进政府采购公平竞争优化营商环境的通知",
        )
        .unwrap();
        assert_eq!(m.level, "规范性文件");
        assert_eq!(m.issuing_body, "财政部");
        assert_eq!(m.doc_number, "财库〔2019〕38号");
        assert_eq!(m.year.as_deref(), Some("2019"));

        // 国办发
        let m = parse_law_meta(
            "《国务院办公厅关于促进政府采购公平竞争优化营商环境的通知》（国办发〔2019〕51号）",
            "关于促进政府采购公平竞争优化营商环境的通知",
        )
        .unwrap();
        assert_eq!(m.issuing_body, "国务院办公厅");
        assert_eq!(m.doc_number, "国办发〔2019〕51号");
    }

    #[test]
    fn test_parse_law_meta_infer_level() {
        // 无文号：按名称后缀推断
        let m = parse_law_meta("《中华人民共和国政府采购法》第五十二条", "政府采购法").unwrap();
        assert_eq!(m.level, "法律");
        assert!(m.doc_number.is_empty());

        let m = parse_law_meta("《政府采购需求管理办法》第九条", "政府采购需求管理办法").unwrap();
        assert_eq!(m.level, "部门规章");

        let m = parse_law_meta("《政府采购框架协议编制指南》", "政府采购框架协议编制指南").unwrap();
        assert_eq!(m.level, "规范性文件");

        let m = parse_law_meta("《中华人民共和国民法典》", "民法典").unwrap();
        assert_eq!(m.level, "法律");
    }

    #[test]
    fn test_law_meta_flow_into_entity() {
        // 端到端：脏 basis → LawArticleEntity 携带 meta
        let cand = Candidate {
            candidate_id: "c1".to_string(),
            risk_id: "risk_001".to_string(),
            severity: "high".to_string(),
            risk_type: "品牌指定".to_string(),
            legal_basis: vec![
                "《政府采购货物和服务招标投标管理办法》（财政部令第87号）第七十七条".to_string(),
            ],
            case_refs: vec![],
            source_quote: "".to_string(),
            reason: "".to_string(),
            suggestion: "".to_string(),
            confidence: 0.9,
        };
        let empty = HashSet::new();
        let res = extract_and_dedup(vec![cand], &empty, "batch-test");
        let law = &res[0].laws[0];
        assert_eq!(law.law_name, "政府采购货物和服务招标投标管理办法");
        let meta = law.meta.as_ref().unwrap();
        assert_eq!(meta.level, "部门规章");
        assert_eq!(meta.doc_number, "财政部令第87号");
        assert_eq!(law.article_no.as_deref(), Some("第77条"));
    }

    #[test]
    fn test_new_attributes_short_name_effective_date_summary() {
        let cand = Candidate {
            candidate_id: "c2".to_string(),
            risk_id: "risk_002".to_string(),
            severity: "high".to_string(),
            risk_type: "地域歧视".to_string(),
            legal_basis: vec![
                "《中华人民共和国政府采购法实施条例》（国务院令第658号）第二十条".to_string(),
            ],
            case_refs: vec!["财政部投诉处理决定第XX号(2023)".to_string()],
            source_quote: "投标人必须在东莞市设有常驻服务机构".to_string(),
            reason: "要求在东莞设立常驻服务机构构成地域限制".to_string(),
            suggestion: "删除地域限制".to_string(),
            confidence: 0.95,
        };
        let empty = HashSet::new();
        let res = extract_and_dedup(vec![cand], &empty, "batch-test");
        let d = &res[0];

        // Law: short_name 去掉"中华人民共和国"前缀
        let law = &d.laws[0];
        assert_eq!(law.short_name, "政府采购法实施条例");
        // Law: effective_date 从文号年份推断
        let meta = law.meta.as_ref().unwrap();
        assert_eq!(meta.effective_date, "");
        // Law: status 默认 effective
        assert_eq!(meta.status, "effective");
        // Article: summary 不再承载单次审核 reason（留空，避免污染共享条款）
        assert!(law.summary.is_empty());
        // 处置经验：reason/suggestion 落 risk_experience（不因 case_refs 存在与否而变）
        assert_eq!(d.risk_experience.reason, "要求在东莞设立常驻服务机构构成地域限制");
        assert_eq!(d.risk_experience.suggestion, "删除地域限制");

        // Case: full_text 为空（无数据来源）、summary 不再拼 reason/suggestion
        let case = &d.cases[0];
        assert!(case.full_text.is_empty());
        assert!(case.summary.is_empty());
        assert_eq!(case.case_type, "投诉处理决定");
    }

    #[test]
    fn test_experience_carries_reason_suggestion_without_case() {
        // 无 case_refs 时 suggestion 依然随经验落库（与案例解耦）
        let cand = Candidate {
            candidate_id: "c3".to_string(),
            risk_id: "risk_003".to_string(),
            severity: "medium".to_string(),
            risk_type: "评分标准倾向".to_string(),
            legal_basis: vec!["《政府采购法实施条例》第二十条".to_string()],
            case_refs: vec![],
            source_quote: "评分标准明显偏向特定品牌".to_string(),
            reason: "评分指标与特定品牌参数强绑定".to_string(),
            suggestion: "重新设计评分维度，去除品牌可识别属性".to_string(),
            confidence: 0.88,
        };
        let empty = HashSet::new();
        let d = &extract_and_dedup(vec![cand], &empty, "batch-test")[0];

        assert!(d.cases.is_empty(), "无案例引用时不应生成 Case 节点");
        assert_eq!(d.risk_experience.source_quote, "评分标准明显偏向特定品牌");
        assert_eq!(d.risk_experience.reason, "评分指标与特定品牌参数强绑定");
        assert_eq!(d.risk_experience.suggestion, "重新设计评分维度，去除品牌可识别属性");
        assert_eq!(d.risk_experience.confidence, 0.88);
        assert!(d.risk_experience.experience_id.starts_with("exp_"), "经验节点 ID 确定性生成");
        // 同风险类型同候选、同文档 → experience_id 稳定（重投幂等）
        assert_eq!(
            d.risk_experience.experience_id,
            gen_experience_id(&d.risk.id, "c3", "batch-test")
        );
        // ProhibitionRule 为稳定禁止模式文案（非逐审核临时 reason）
        assert_eq!(d.rules[0].content, "禁止评分标准倾向类行为");
    }

    #[test]
    fn test_experience_id_salted_by_batch_key() {
        // 跨文档碰撞回归：同风险类型、同候选序号、不同文档（batch_key）→ experience_id 必不同
        let mk = |candidate_id: String| Candidate {
            candidate_id,
            risk_id: "r".to_string(),
            severity: "high".to_string(),
            risk_type: "地域歧视".to_string(),
            legal_basis: vec!["《政府采购法实施条例》第二十条".to_string()],
            case_refs: vec![],
            source_quote: "投标人必须有东莞常驻机构".to_string(),
            reason: "地域限制".to_string(),
            suggestion: "删除".to_string(),
            confidence: 0.9,
        };
        let empty = HashSet::new();
        // 场景：标书A、标书B 各审出同类型风险，且都落在会话内第 5 个（candidate_id=R_005）
        let a = &extract_and_dedup(
            vec![mk("R_005".to_string())],
            &empty,
            "doc-A.pdf",
        )[0];
        let b = &extract_and_dedup(
            vec![mk("R_005".to_string())],
            &empty,
            "doc-B.pdf",
        )[0];
        assert_ne!(
            a.risk_experience.experience_id,
            b.risk_experience.experience_id,
            "不同文档同类型同序号必须产出不同 experience_id，否则 MERGE 会静默吞掉第二次经验"
        );
        // 同文档重投 → 仍幂等
        let a2 = &extract_and_dedup(
            vec![mk("R_005".to_string())],
            &empty,
            "doc-A.pdf",
        )[0];
        assert_eq!(
            a.risk_experience.experience_id,
            a2.risk_experience.experience_id,
            "同文档重投必须幂等"
        );
    }

    #[test]
    fn test_rule_id_stable_across_reviews() {
        // 同一 risk_type、不同 reason 的两次审核 → 归并到同一条 ProhibitionRule
        let mk = |reason: String| Candidate {
            candidate_id: "c".to_string(),
            risk_id: "r".to_string(),
            severity: "high".to_string(),
            risk_type: "地域歧视".to_string(),
            legal_basis: vec!["《政府采购法实施条例》第二十条".to_string()],
            case_refs: vec![],
            source_quote: "q".to_string(),
            reason,
            suggestion: "删除".to_string(),
            confidence: 0.9,
        };
        let empty = HashSet::new();
        let a = &extract_and_dedup(
            vec![mk("第一次不同的论证口径".to_string())],
            &empty,
            "batch-test",
        )[0];
        let b = &extract_and_dedup(
            vec![mk("第二次再次不同的论证口径".to_string())],
            &empty,
            "batch-test",
        )[0];
        assert_eq!(a.rules[0].content, b.rules[0].content);
        assert_eq!(a.rules[0].rule_id, b.rules[0].rule_id, "content 稳定 → rule_id 稳定 → 跨审核归并");
        assert_eq!(a.rules[0].content, "禁止地域歧视类行为");
    }

    #[test]
    fn test_risk_id_consistent() {
        let id1 = gen_risk_id("品牌指定");
        let id2 = gen_risk_id("品牌指定");
        assert_eq!(id1, id2);
        assert!(id1.starts_with("risk_"));
        assert_ne!(gen_risk_id("品牌指定"), gen_risk_id("资格条件"));
    }

    #[test]
    fn test_law_id_consistent() {
        // 相同输入永远生成相同 ID
        let id1 = gen_law_id("政府采购法实施条例");
        let id2 = gen_law_id("政府采购法实施条例");
        assert_eq!(id1, id2);
        assert!(id1.starts_with("law_"));
        assert_eq!(id1.len(), 12); // law_ + 8位
    }

    #[test]
    fn test_dedup_logic() {
        let cand = Candidate {
            candidate_id: "c1".to_string(),
            risk_id: "risk_001".to_string(),
            severity: "high".to_string(),
            risk_type: "品牌指定".to_string(),
            legal_basis: vec!["《政府采购法实施条例》第二十条".to_string()],
            case_refs: vec![],
            source_quote: "".to_string(),
            reason: "".to_string(),
            suggestion: "".to_string(),
            confidence: 0.9,
        };

        // 空库 → New
        let empty = HashSet::new();
        let res = extract_and_dedup(vec![cand.clone()], &empty, "batch-test");
        assert_eq!(res[0].decision, Decision::New);

        // 库中已有 → Exists
        let law_id = gen_law_id("政府采购法实施条例");
        let mut existing = HashSet::new();
        existing.insert(law_id);
        let res = extract_and_dedup(vec![cand], &existing, "batch-test");
        assert_eq!(res[0].decision, Decision::Exists);
    }
}