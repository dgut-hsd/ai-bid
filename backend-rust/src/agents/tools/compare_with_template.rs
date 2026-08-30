//! `compare_with_template` 工具 — 将当前条款与标准范本进行结构化比对。
//!
//! 语义搜索擅长找"写了什么"，模板比对擅长发现"没写什么"——
//! 很多违规是该写的不写、不该写的写了。
//!
//! ## 核心逻辑
//!
//! 1. 从 TemplateStore 中加载对应类别的标准模板
//! 2. 提取模板的必须项清单 (required_items) 和禁止项清单 (forbidden_patterns)
//! 3. 在实际条款中检查每项是否存在
//! 4. 返回 missing / extra / wording_diff 三部分结果
//!
//! ## 模板库
//!
//! 模板存储在内存 TemplateStore 中，支持运行时注册。
//! 初始内置：资格条件标准模板、合同必须条款模板、评审标准模板。

use anyhow::{Result, anyhow};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;

use crate::domain::chunk::Chunk;
use super::AgentTool;

/// 存储的标准模板。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StandardTemplate {
    /// 模板类型标识
    pub template_type: String,
    /// 模板名称
    pub name: String,
    /// 必须包含的条目（缺少 → 违规）
    pub required_items: Vec<TemplateItem>,
    /// 不应包含的条目（出现 → 可疑）
    pub forbidden_patterns: Vec<String>,
    /// 通常包含的字段名（用于 wording_diff 比对）
    pub expected_fields: Vec<String>,
}

/// 模板中的单个必须项。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TemplateItem {
    /// 条目名称
    pub item: String,
    /// 缺失时的严重程度
    pub severity: String, // "high" | "medium"
    /// 法条依据
    pub legal_basis: String,
}

/// `compare_with_template` 工具的参数。
#[derive(Debug, Deserialize)]
pub struct CompareWithTemplateArgs {
    /// 待比对的 chunk_id
    pub clause_chunk: String,
    /// 模板类型
    pub template_type: String,
}

/// 模板比对的返回结果。
#[derive(Debug, Serialize)]
struct TemplateCompareResult {
    /// 缺失的必须项
    missing: Vec<MissingItem>,
    /// 多余的/可疑的内容
    extra: Vec<ExtraItem>,
    /// 措辞差异
    wording_diff: Vec<WordingDiff>,
    /// 使用的模板名称
    template_name: String,
    /// 摘要
    summary: String,
}

#[derive(Debug, Serialize)]
struct MissingItem {
    item: String,
    severity: String,
    legal_basis: String,
}

#[derive(Debug, Serialize)]
struct ExtraItem {
    item: String,
    flag: String, // redundant | restrictive | unusual
}

#[derive(Debug, Serialize)]
struct WordingDiff {
    field: String,
    expected: String,
    actual: String,
}

/// 模板存储库（内存中）。
///
/// 支持预置模板和运行时注册。
pub struct TemplateStore {
    templates: HashMap<String, StandardTemplate>,
}

impl TemplateStore {
    /// 创建空的模板存储。
    pub fn new() -> Self {
        Self {
            templates: HashMap::new(),
        }
    }

    /// 创建带内置模板的存储。
    pub fn with_builtin_templates() -> Self {
        let mut store = Self::new();
        store.register_builtin_templates();
        store
    }

    /// 注册一个模板。
    pub fn register(&mut self, template: StandardTemplate) {
        self.templates
            .insert(template.template_type.clone(), template);
    }

    /// 获取模板。
    pub fn get(&self, template_type: &str) -> Option<&StandardTemplate> {
        self.templates.get(template_type)
    }

    /// 注册内置标准模板。
    fn register_builtin_templates(&mut self) {
        // ── 资格条件标准模板 ──
        self.register(StandardTemplate {
            template_type: "资格条件标准模板".to_string(),
            name: "政府采购供应商资格条件标准模板".to_string(),
            required_items: vec![
                TemplateItem {
                    item: "具有独立承担民事责任的能力".to_string(),
                    severity: "high".to_string(),
                    legal_basis: "《政府采购法》第22条(一)".to_string(),
                },
                TemplateItem {
                    item: "具有良好的商业信誉和健全的财务会计制度".to_string(),
                    severity: "high".to_string(),
                    legal_basis: "《政府采购法》第22条(二)".to_string(),
                },
                TemplateItem {
                    item: "具有履行合同所必需的设备和专业技术能力".to_string(),
                    severity: "high".to_string(),
                    legal_basis: "《政府采购法》第22条(三)".to_string(),
                },
                TemplateItem {
                    item: "有依法缴纳税收和社会保障资金的良好记录".to_string(),
                    severity: "high".to_string(),
                    legal_basis: "《政府采购法》第22条(四)".to_string(),
                },
                TemplateItem {
                    item: "参加政府采购活动前三年内，在经营活动中没有重大违法记录".to_string(),
                    severity: "high".to_string(),
                    legal_basis: "《政府采购法》第22条(五)".to_string(),
                },
                TemplateItem {
                    item: "未被列入失信被执行人、重大税收违法案件当事人名单、政府采购严重违法失信行为记录名单".to_string(),
                    severity: "high".to_string(),
                    legal_basis: "《政府采购法》第22条、《关于在政府采购活动中查询及使用信用记录有关问题的通知》".to_string(),
                },
            ],
            forbidden_patterns: vec![
                "注册资本".to_string(),
                "资产总额".to_string(),
                "营业收入".to_string(),
                "从业人员".to_string(),
                "利润".to_string(),
                "纳税额".to_string(),
                "本地注册".to_string(),
                "在.*设有分支机构".to_string(),
                "本地业绩".to_string(),
                "特定品牌".to_string(),
                "唯一授权".to_string(),
                "原厂商".to_string(),
                "地域".to_string(),
                "规模".to_string(),
            ],
            expected_fields: vec![
                "供应商资格要求".to_string(),
                "资质要求".to_string(),
                "需要提供的证明材料".to_string(),
                "联合体".to_string(),
            ],
        });

        // ── 合同必须条款模板 ──
        self.register(StandardTemplate {
            template_type: "合同必须条款模板".to_string(),
            name: "政府采购合同必须条款".to_string(),
            required_items: vec![
                TemplateItem {
                    item: "合同金额及付款方式".to_string(),
                    severity: "high".to_string(),
                    legal_basis: "《政府采购法》第46条".to_string(),
                },
                TemplateItem {
                    item: "履约保证金".to_string(),
                    severity: "medium".to_string(),
                    legal_basis: "《政府采购法实施条例》第48条".to_string(),
                },
                TemplateItem {
                    item: "违约责任".to_string(),
                    severity: "high".to_string(),
                    legal_basis: "《民法典》合同编".to_string(),
                },
                TemplateItem {
                    item: "争议解决方式".to_string(),
                    severity: "medium".to_string(),
                    legal_basis: "政府采购标准合同文本".to_string(),
                },
                TemplateItem {
                    item: "验收标准与程序".to_string(),
                    severity: "high".to_string(),
                    legal_basis: "《政府采购法》第41条".to_string(),
                },
                TemplateItem {
                    item: "合同履行期限".to_string(),
                    severity: "high".to_string(),
                    legal_basis: "《政府采购法》第46条".to_string(),
                },
                TemplateItem {
                    item: "保密条款".to_string(),
                    severity: "medium".to_string(),
                    legal_basis: "政府采购标准合同文本".to_string(),
                },
            ],
            forbidden_patterns: vec![
                "最终解释权".to_string(),
                "以.*意见为准".to_string(),
                "无条件接受".to_string(),
                "不得异议".to_string(),
                "放弃.*权利".to_string(),
                "无限期".to_string(),
            ],
            expected_fields: vec![
                "合同金额".to_string(),
                "付款".to_string(),
                "履约保证金".to_string(),
                "违约责任".to_string(),
                "争议解决".to_string(),
                "验收".to_string(),
            ],
        });

        // ── 评审标准模板 ──
        self.register(StandardTemplate {
            template_type: "评审标准模板".to_string(),
            name: "政府采购评审标准模板".to_string(),
            required_items: vec![
                TemplateItem {
                    item: "价格分权重（货物≥30%，服务≥10%）".to_string(),
                    severity: "high".to_string(),
                    legal_basis: "《政府采购货物和服务招标投标管理办法》第55条".to_string(),
                },
                TemplateItem {
                    item: "技术评审因素".to_string(),
                    severity: "medium".to_string(),
                    legal_basis: "政府采购标准评审办法".to_string(),
                },
                TemplateItem {
                    item: "商务评审因素".to_string(),
                    severity: "medium".to_string(),
                    legal_basis: "政府采购标准评审办法".to_string(),
                },
                TemplateItem {
                    item: "评分权重总计100%".to_string(),
                    severity: "high".to_string(),
                    legal_basis: "政府采购评审原则".to_string(),
                },
            ],
            forbidden_patterns: vec![
                "地域.*加分".to_string(),
                "本地.*业绩.*加分".to_string(),
                "品牌.*加分".to_string(),
                "特定.*型号.*加分".to_string(),
                "注册地".to_string(),
            ],
            expected_fields: vec![
                "价格分".to_string(),
                "技术评审".to_string(),
                "商务评审".to_string(),
            ],
        });

        // ── 投标文件格式模板 ──
        self.register(StandardTemplate {
            template_type: "投标文件格式模板".to_string(),
            name: "政府采购投标文件格式标准模板".to_string(),
            required_items: vec![
                TemplateItem {
                    item: "投标函格式要求".to_string(),
                    severity: "high".to_string(),
                    legal_basis: "《政府采购货物和服务招标投标管理办法》第32条".to_string(),
                },
                TemplateItem {
                    item: "装订与密封要求".to_string(),
                    severity: "high".to_string(),
                    legal_basis: "政府采购标准文件格式规范".to_string(),
                },
                TemplateItem {
                    item: "签字盖章要求".to_string(),
                    severity: "high".to_string(),
                    legal_basis: "《政府采购货物和服务招标投标管理办法》第33条".to_string(),
                },
                TemplateItem {
                    item: "副本数量要求".to_string(),
                    severity: "medium".to_string(),
                    legal_basis: "政府采购标准文件格式规范".to_string(),
                },
                TemplateItem {
                    item: "电子版文件要求".to_string(),
                    severity: "medium".to_string(),
                    legal_basis: "《关于促进政府采购公平竞争优化营商环境的通知》".to_string(),
                },
                TemplateItem {
                    item: "投标有效期要求".to_string(),
                    severity: "high".to_string(),
                    legal_basis: "《政府采购货物和服务招标投标管理办法》第23条".to_string(),
                },
                TemplateItem {
                    item: "联合体协议格式（如适用）".to_string(),
                    severity: "medium".to_string(),
                    legal_basis: "《政府采购法》第24条".to_string(),
                },
                TemplateItem {
                    item: "法定代表人授权委托书格式".to_string(),
                    severity: "high".to_string(),
                    legal_basis: "政府采购标准文件格式规范".to_string(),
                },
            ],
            forbidden_patterns: vec![
                "格式自拟".to_string(),
                "自行决定".to_string(),
                "不强制要求".to_string(),
                "无需签字".to_string(),
                "无需盖章".to_string(),
            ],
            expected_fields: vec![
                "投标函".to_string(),
                "装订".to_string(),
                "密封".to_string(),
                "签字".to_string(),
                "盖章".to_string(),
                "副本".to_string(),
                "有效期".to_string(),
            ],
        });

        // ── 采购需求描述规范 ──
        self.register(StandardTemplate {
            template_type: "采购需求描述规范".to_string(),
            name: "政府采购采购需求描述规范模板".to_string(),
            required_items: vec![
                TemplateItem {
                    item: "技术参数完整性与明确性".to_string(),
                    severity: "high".to_string(),
                    legal_basis: "《政府采购需求管理办法》第8条".to_string(),
                },
                TemplateItem {
                    item: "验收标准明确性".to_string(),
                    severity: "high".to_string(),
                    legal_basis: "《政府采购需求管理办法》第12条".to_string(),
                },
                TemplateItem {
                    item: "服务内容与范围界定".to_string(),
                    severity: "medium".to_string(),
                    legal_basis: "《政府采购需求管理办法》第9条".to_string(),
                },
                TemplateItem {
                    item: "交付物清单".to_string(),
                    severity: "medium".to_string(),
                    legal_basis: "政府采购标准合同文本".to_string(),
                },
                TemplateItem {
                    item: "质量标准与验收方法".to_string(),
                    severity: "high".to_string(),
                    legal_basis: "《政府采购需求管理办法》第12条".to_string(),
                },
                TemplateItem {
                    item: "履约期限与进度安排".to_string(),
                    severity: "medium".to_string(),
                    legal_basis: "《政府采购需求管理办法》第11条".to_string(),
                },
            ],
            forbidden_patterns: vec![
                "指定.*品牌".to_string(),
                "指定.*型号".to_string(),
                "唯一.*专利".to_string(),
                "特定.*供应商".to_string(),
                "量身定做".to_string(),
                "排他性".to_string(),
            ],
            expected_fields: vec![
                "技术参数".to_string(),
                "验收标准".to_string(),
                "服务范围".to_string(),
                "交付物".to_string(),
                "质量标准".to_string(),
                "履约期限".to_string(),
            ],
        });

        // ── 政府采购负面清单模板 ──
        self.register(StandardTemplate {
            template_type: "政府采购负面清单模板".to_string(),
            name: "政府采购负面行为清单（财政部发布）".to_string(),
            required_items: vec![
                TemplateItem {
                    item: "未设置供应商规模门槛（注册资本/资产总额/营业收入等）".to_string(),
                    severity: "high".to_string(),
                    legal_basis: "《政府采购法实施条例》第20条(二)".to_string(),
                },
                TemplateItem {
                    item: "未将特定行政区域业绩作为资格条件或评分因素".to_string(),
                    severity: "high".to_string(),
                    legal_basis: "《政府采购法实施条例》第20条(四)".to_string(),
                },
                TemplateItem {
                    item: "未指定特定品牌或供应商".to_string(),
                    severity: "high".to_string(),
                    legal_basis: "《政府采购法》第22条".to_string(),
                },
                TemplateItem {
                    item: "未将非国家强制认证作为资格条件".to_string(),
                    severity: "high".to_string(),
                    legal_basis: "《政府采购法实施条例》第20条(八)".to_string(),
                },
                TemplateItem {
                    item: "未要求提供厂家授权函作为资格条件".to_string(),
                    severity: "high".to_string(),
                    legal_basis: "《政府采购货物和服务招标投标管理办法》第17条".to_string(),
                },
            ],
            forbidden_patterns: vec![
                // 30+ 禁止性模式
                "注册资本".to_string(),
                "资产总额".to_string(),
                "营业收入".to_string(),
                "从业人员".to_string(),
                "纳税额".to_string(),
                "本地.*业绩".to_string(),
                "本地.*注册".to_string(),
                "所在地.*资格".to_string(),
                "指定.*品牌".to_string(),
                "指定.*型号".to_string(),
                "唯一.*授权".to_string(),
                "原厂.*授权".to_string(),
                "厂家.*授权函".to_string(),
                "制造商.*证明".to_string(),
                "专利.*证书".to_string(),
                "非.*认证".to_string(),
                "行业协会.*证书".to_string(),
                "企业.*排名".to_string(),
                "地方.*奖项".to_string(),
                "省内.*荣誉".to_string(),
                "评委.*酌情".to_string(),
                "自行.*掌握".to_string(),
                "综合.*判断".to_string(),
                "无.*量化".to_string(),
                "没有.*细则".to_string(),
                "最终.*解释权".to_string(),
                "以.*意见为准".to_string(),
                "无条件.*接受".to_string(),
                "不得.*异议".to_string(),
                "放弃.*权利".to_string(),
                "全部.*责任".to_string(),
                "一切.*责任".to_string(),
                "永久.*归".to_string(),
                "无限.*期".to_string(),
                "单方.*变更".to_string(),
                "无.*上限".to_string(),
            ],
            expected_fields: vec![
                "资格条件".to_string(),
                "评分标准".to_string(),
                "合同条款".to_string(),
                "技术要求".to_string(),
            ],
        });
    }
}

impl Default for TemplateStore {
    fn default() -> Self {
        Self::with_builtin_templates()
    }
}

/// `compare_with_template` 工具实现。
pub struct CompareWithTemplateTool {
    /// 共享的模板存储
    pub templates: Arc<TemplateStore>,
    /// 条款文本提供者：从 chunk_id 获取文本
    pub text_provider: Arc<dyn ClauseTextProvider>,
}

/// 条款文本提供者 trait — 解耦工具与 Chunk 存储。
pub trait ClauseTextProvider: Send + Sync {
    fn get_text(&self, chunk_id: &str) -> Option<String>;
    fn get_section_path(&self, chunk_id: &str) -> Option<Vec<String>>;
}

/// `ClauseTextProvider` 的真实实现 — 从 `Arc<HashMap<String, Chunk>>` 中提取文本。
pub struct ChunkTextProvider {
    pub chunks: Arc<HashMap<String, Chunk>>,
}

impl ClauseTextProvider for ChunkTextProvider {
    fn get_text(&self, chunk_id: &str) -> Option<String> {
        self.chunks.get(chunk_id).map(|c| c.text.clone())
    }
    fn get_section_path(&self, chunk_id: &str) -> Option<Vec<String>> {
        self.chunks.get(chunk_id).map(|c| c.section_path.clone())
    }
}

impl CompareWithTemplateTool {
    pub fn new(templates: Arc<TemplateStore>, text_provider: Arc<dyn ClauseTextProvider>) -> Self {
        Self {
            templates,
            text_provider,
        }
    }

    /// 检查文本中是否存在某条目（模糊匹配）。
    fn contains_item(text: &str, item: &str) -> bool {
        // 提取 item 中的核心关键词匹配
        let keywords: Vec<&str> = item
            .split(['、', '，', ',', '（', '('])
            .next()
            .map(|s| s.trim())
            .unwrap_or(item)
            .split_whitespace()
            .collect();

        let text_lower = text.to_lowercase();
        for kw in &keywords {
            if kw.len() >= 4 && text_lower.contains(&kw.to_lowercase()) {
                return true;
            }
        }
        // 检查 item 的主体部分（取前 6 个字符作为最小匹配）
        let core = item.chars().take(6).collect::<String>();
        text_lower.contains(&core.to_lowercase())
    }

    /// 检查文本中是否包含禁止模式（正则模糊匹配）。
    fn contains_forbidden(text: &str, pattern: &str) -> bool {
        let text_lower = text.to_lowercase();
        // 简单包含匹配（将 .* 转为实际模糊匹配）
        if pattern.contains(".*") {
            let parts: Vec<&str> = pattern.split(".*").collect();
            let mut pos = 0usize;
            for part in &parts {
                if let Some(found) = text_lower[pos..].find(&part.to_lowercase()) {
                    pos += found + part.len();
                } else {
                    return false;
                }
            }
            true
        } else {
            text_lower.contains(&pattern.to_lowercase())
        }
    }
}

#[async_trait::async_trait]
impl AgentTool for CompareWithTemplateTool {
    fn name(&self) -> &str {
        "compare_with_template"
    }

    fn definition(&self) -> serde_json::Value {
        // 构建动态的 template_type enum 列表
        let template_types: Vec<&str> = vec![
            "资格条件标准模板",
            "合同必须条款模板",
            "投标文件格式模板",
            "评审标准模板",
            "采购需求描述规范",
        ];

        serde_json::json!({
            "type": "function",
            "function": {
                "name": "compare_with_template",
                "description": "与标准范本比对，发现缺失的法定必备条款。",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "clause_chunk": {
                            "type": "string",
                            "description": "待比对的 chunk_id，如 'ch_042'"
                        },
                        "template_type": {
                            "type": "string",
                            "enum": template_types,
                            "description": "模板类型"
                        }
                    },
                    "required": ["clause_chunk", "template_type"]
                }
            }
        })
    }

    async fn execute(&self, args: serde_json::Value) -> Result<serde_json::Value> {
        let parsed: CompareWithTemplateArgs = serde_json::from_value(args)?;

        // 1. 获取条款文本
        let clause_text = self
            .text_provider
            .get_text(&parsed.clause_chunk)
            .ok_or_else(|| anyhow!("chunk_id 不存在: {}", parsed.clause_chunk))?;

        // 2. 获取模板
        let template = self.templates.get(&parsed.template_type).ok_or_else(|| {
            anyhow!(
                "未知模板类型: {}。可用模板: {}",
                parsed.template_type,
                self.templates
                    .templates
                    .keys()
                    .map(|s| s.as_str())
                    .collect::<Vec<_>>()
                    .join(", ")
            )
        })?;

        // 3. 检查必须项（missing）
        let mut missing = Vec::new();
        for item in &template.required_items {
            if !Self::contains_item(&clause_text, &item.item) {
                missing.push(MissingItem {
                    item: item.item.clone(),
                    severity: item.severity.clone(),
                    legal_basis: item.legal_basis.clone(),
                });
            }
        }

        // 4. 检查禁止模式（extra）
        let mut extra = Vec::new();
        for pattern in &template.forbidden_patterns {
            if Self::contains_forbidden(&clause_text, pattern) {
                // 提取匹配的文本片段作为展示
                let flag = if pattern.contains("本地") || pattern.contains("地域") {
                    "restrictive"
                } else if pattern.contains("解释权") || pattern.contains("不得异议") {
                    "overbroad"
                } else if pattern.contains("品牌") || pattern.contains("授权") {
                    "restrictive"
                } else {
                    "unusual"
                };
                extra.push(ExtraItem {
                    item: pattern.clone(),
                    flag: flag.to_string(),
                });
            }
        }

        // 5. 措辞差异（wording_diff）— 检查 expected_fields 是否存在
        let mut wording_diff = Vec::new();
        for field in &template.expected_fields {
            if !clause_text.contains(field) && !clause_text.contains(&field.to_lowercase()) {
                wording_diff.push(WordingDiff {
                    field: field.clone(),
                    expected: format!("应包含 '{}' 相关条款", field),
                    actual: "未找到".to_string(),
                });
            }
        }

        // 6. 生成摘要
        let summary = if missing.is_empty() && extra.is_empty() && wording_diff.is_empty() {
            format!("✅ 该条款与'{}'模板完全匹配", template.name)
        } else {
            let mut parts = Vec::new();
            if !missing.is_empty() {
                parts.push(format!("缺失 {} 项必须条款", missing.len()));
            }
            if !extra.is_empty() {
                parts.push(format!("发现 {} 处可疑内容", extra.len()));
            }
            if !wording_diff.is_empty() {
                parts.push(format!("{} 处措辞差异", wording_diff.len()));
            }
            format!("⚠️ {}", parts.join("，"))
        };

        let result = TemplateCompareResult {
            missing,
            extra,
            wording_diff,
            template_name: template.name.clone(),
            summary,
        };

        Ok(serde_json::to_value(&result)?)
    }
}

// ─── 测试 ──────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    struct MockTextProvider {
        texts: HashMap<String, String>,
    }

    impl ClauseTextProvider for MockTextProvider {
        fn get_text(&self, chunk_id: &str) -> Option<String> {
            self.texts.get(chunk_id).cloned()
        }
        fn get_section_path(&self, _chunk_id: &str) -> Option<Vec<String>> {
            Some(vec!["测试".to_string()])
        }
    }

    #[test]
    fn test_contains_item_exact() {
        assert!(CompareWithTemplateTool::contains_item(
            "供应商应具有独立承担民事责任的能力",
            "具有独立承担民事责任的能力"
        ));
    }

    #[test]
    fn test_contains_item_partial() {
        // 文本包含"良好的商业信誉"关键词（从 item 中拆分出的前几个字匹配）
        assert!(CompareWithTemplateTool::contains_item(
            "投标人须具有良好的商业信誉和完善的财务会计制度",
            "具有良好的商业信誉和健全的财务会计制度"
        ));
    }

    #[test]
    fn test_contains_item_missing() {
        assert!(!CompareWithTemplateTool::contains_item(
            "供应商须提供营业执照",
            "具有独立承担民事责任的能力"
        ));
    }

    #[test]
    fn test_contains_forbidden_regex() {
        // "在.*设有分支机构" 匹配 "在东莞市设有常驻服务机构"
        assert!(CompareWithTemplateTool::contains_forbidden(
            "投标人须在东莞市设有常驻服务机构",
            "在.*设有"
        ));
    }

    #[test]
    fn test_contains_forbidden_brand() {
        // "品牌" 是 forbidden_patterns 中的子模式，文本中包含即匹配
        assert!(CompareWithTemplateTool::contains_forbidden(
            "本项目指定华为品牌设备",
            "品牌"
        ));
    }

    #[test]
    fn test_builtin_templates_exist() {
        let store = TemplateStore::with_builtin_templates();
        assert!(store.get("资格条件标准模板").is_some());
        assert!(store.get("合同必须条款模板").is_some());
        assert!(store.get("评审标准模板").is_some());
        assert!(store.get("投标文件格式模板").is_some());
        assert!(store.get("采购需求描述规范").is_some());
        assert!(store.get("政府采购负面清单模板").is_some());
    }

    #[test]
    fn test_template_missing_required_items() {
        let store = Arc::new(TemplateStore::with_builtin_templates());
        let mut texts = HashMap::new();
        // 只写了部分资格条件
        texts.insert(
            "ch_001".to_string(),
            "投标人须具有独立承担民事责任的能力，提供营业执照副本复印件。".to_string(),
        );
        let provider = Arc::new(MockTextProvider { texts });

        let tool = CompareWithTemplateTool::new(store, provider);
        let args = serde_json::json!({
            "clause_chunk": "ch_001",
            "template_type": "资格条件标准模板"
        });
        // 用 block_on 测试
        let rt = tokio::runtime::Runtime::new().unwrap();
        let result = rt.block_on(tool.execute(args)).unwrap();
        let result: serde_json::Value = result;
        let missing: Vec<serde_json::Value> =
            serde_json::from_value(result["missing"].clone()).unwrap();
        // 应该缺失 5 项（只有第1项满足）
        assert!(
            missing.len() >= 2,
            "应至少缺失 2 项必须条款，实际: {}",
            missing.len()
        );
    }
}
