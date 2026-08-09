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

// --------------------------
// 单元5：主函数 - 实体拆分 + 查重
// --------------------------
pub fn extract_and_dedup(
    candidates: Vec<Candidate>,
    existing_law_ids: &HashSet<String>,
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

            // 拆分所有法律依据
            let laws: Vec<LawArticleEntity> = cand
                .legal_basis
                .iter()
                .map(|basis| {
                    let (law_name, article_no) = parse_law_basis(basis);
                    let law_id = gen_law_id(&law_name);
                    let article_id = article_no
                        .as_ref()
                        .map(|no| gen_article_id(&law_id, no));
                    let meta = parse_law_meta(basis, &law_name);

                    LawArticleEntity {
                        law_id,
                        law_name,
                        article_id,
                        article_no,
                        meta,
                    }
                })
                .collect();

            // 查重判断：只要有一个 law_id 不在库里，就标记为 New
            let has_new_law = laws.iter().any(|law| !existing_law_ids.contains(&law.law_id));
            let decision = if has_new_law {
                Decision::New
            } else {
                Decision::Exists
            };

            EntityDecision {
                candidate_id: cand.candidate_id,
                decision,
                risk,
                laws,
                snippet: cand.source_quote.clone(),
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
        let res = extract_and_dedup(vec![cand], &empty);
        let law = &res[0].laws[0];
        assert_eq!(law.law_name, "政府采购货物和服务招标投标管理办法");
        let meta = law.meta.as_ref().unwrap();
        assert_eq!(meta.level, "部门规章");
        assert_eq!(meta.doc_number, "财政部令第87号");
        assert_eq!(law.article_no.as_deref(), Some("第77条"));
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
        let res = extract_and_dedup(vec![cand.clone()], &empty);
        assert_eq!(res[0].decision, Decision::New);

        // 库中已有 → Exists
        let law_id = gen_law_id("政府采购法实施条例");
        let mut existing = HashSet::new();
        existing.insert(law_id);
        let res = extract_and_dedup(vec![cand], &existing);
        assert_eq!(res[0].decision, Decision::Exists);
    }
}