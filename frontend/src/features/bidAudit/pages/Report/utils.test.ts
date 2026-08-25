import { describe, it, expect } from 'vitest';
import {
  parseMarkdownToSections,
  parseReportSections,
  MOCK_REPORT,
  REPORT_SECTIONS,
} from './utils';
import type { Report } from './types';

// ─── parseMarkdownToSections — 标准 # 标题格式 ───

describe('parseMarkdownToSections — h1 标题格式 (#)', () => {
  it('解析标准 # 标题 Markdown，提取各段落内容', () => {
    const md = `
# 封面
标书智能审核报告
项目名称：2026年度数字校园项目
---
# 基本信息
- 投标单位：智网科技
- 标书总页数：156页
---
# 审核结论
本项目标书整体结构完整。
结论建议：修改后复审。
---
# 问题汇总
| 审核维度 | 严重风险 |
| :--- | :---: |
| 预算合规性 | 2 |
---
# 详细列表
### 1. [严重] 硬件报价超标
问题描述：核心交换机报价超标。
---
# 审核说明
1. 本报告由系统 AI 自动生成。
    `.trim();

    const sections = parseMarkdownToSections(md);

    expect(sections['封面']).toContain('标书智能审核报告');
    expect(sections['基本信息']).toContain('投标单位：智网科技');
    expect(sections['审核结论']).toContain('本项目标书整体结构完整');
    expect(sections['问题汇总']).toContain('预算合规性');
    expect(sections['详细列表']).toContain('硬件报价超标');
    expect(sections['审核说明']).toContain('本报告由系统 AI 自动生成');
  });

  it('段落内容从当前标题行开始到下一标题之前', () => {
    const md = '# 封面\n封面内容\n# 基本信息\n基本信息内容';
    const sections = parseMarkdownToSections(md);

    // 函数把标题行本身也包含在段落内容中
    expect(sections['封面']).toBe('# 封面\n封面内容');
    expect(sections['基本信息']).toBe('# 基本信息\n基本信息内容');
  });

  it('未知的 # 标题被纳入上一个已知段落的 raw content', () => {
    const md = `# 封面
封面内容
# 未知章节
这段内容应被忽略
# 基本信息
基本信息内容`;

    const sections = parseMarkdownToSections(md);
    // "未知章节"不是已知标题，所以其内容归入上一个已知标题（封面）段落
    expect(sections['封面']).toContain('未知章节');
    expect(sections['封面']).toContain('这段内容应被忽略');
    // "基本信息"段落从 "# 基本信息" 开始，不应包含未知章节的内容
    expect(sections['基本信息']).not.toContain('未知章节');
    expect(sections['基本信息']).not.toContain('这段内容应被忽略');
  });
});

// ─── parseMarkdownToSections — 中文编号 ## 标题格式 (后端兼容) ───

describe('parseMarkdownToSections — 中文编号 ## 标题格式', () => {
  const chineseMd = `
标书封面前言

## 一、标书基本信息
- 投标单位：智网科技
- 总页数：156页

## 二、审核结论
本项目标书整体结构完整。
结论建议：修改后复审。

## 三、问题汇总
| 维度 | 数量 |
| --- | --- |
| 预算 | 2 |

## 四、详细问题列表
### 1. [严重] 硬件报价超标
报价超过指导价上限 15%。

## 五、审核说明
本报告由系统 AI 自动生成。
    `.trim();

  it('解析中文编号 ## 标题，正确映射到标准 SectionTitle', () => {
    const sections = parseMarkdownToSections(chineseMd);

    expect(sections['封面']).toContain('标书封面前言');
    expect(sections['基本信息']).toContain('投标单位：智网科技');
    expect(sections['审核结论']).toContain('本项目标书整体结构完整');
    expect(sections['问题汇总']).toContain('预算');
    expect(sections['详细列表']).toContain('硬件报价超标');
    expect(sections['审核说明']).toContain('本报告由系统 AI 自动生成');
  });

  it('中文编号标题之间内容不互相污染', () => {
    const sections = parseMarkdownToSections(chineseMd);

    expect(sections['基本信息']).not.toContain('审核结论');
    expect(sections['审核结论']).not.toContain('问题汇总');
    expect(sections['详细列表']).not.toContain('审核说明');
  });

  it('封面内容为第一个 ## 标题之前的所有文本', () => {
    const sections = parseMarkdownToSections(chineseMd);

    // 封面是全文第一个 ## 标题之前的前言
    expect(sections['封面']).toBe('标书封面前言');
  });

  it('映射 "详细问题列表" → "详细列表"', () => {
    const sections = parseMarkdownToSections(chineseMd);

    expect(sections['详细列表']).toContain('硬件报价超标');
  });
});

// ─── parseMarkdownToSections — 边界情况 ───

describe('parseMarkdownToSections — 边界情况', () => {
  it('空字符串返回所有段落为空字符串', () => {
    const sections = parseMarkdownToSections('');

    REPORT_SECTIONS.forEach((key) => {
      expect(sections[key]).toBe('');
    });
  });

  it('无匹配标题时返回所有段落为空字符串', () => {
    const md = '# 无关标题\n一些内容\n## 另一个无关标题\n更多内容';

    const sections = parseMarkdownToSections(md);

    REPORT_SECTIONS.forEach((key) => {
      expect(sections[key]).toBe('');
    });
  });

  it('只有标题没有内容时，段落内容为标题行本身', () => {
    const md = '# 封面\n# 基本信息\n# 审核结论\n# 问题汇总\n# 详细列表\n# 审核说明';

    const sections = parseMarkdownToSections(md);

    expect(sections['封面']).toBe('# 封面');
    expect(sections['基本信息']).toBe('# 基本信息');
    expect(sections['审核结论']).toBe('# 审核结论');
    expect(sections['问题汇总']).toBe('# 问题汇总');
    expect(sections['详细列表']).toBe('# 详细列表');
    expect(sections['审核说明']).toBe('# 审核说明');
  });

  it('标题前后有多余空格时仍正确匹配', () => {
    const md = '# 封面  \n封面内容\n# 基本信息  \n基本信息内容';

    const sections = parseMarkdownToSections(md);
    expect(sections['封面']).toContain('封面内容');
    expect(sections['基本信息']).toContain('基本信息内容');
  });
});

// ─── parseReportSections ───

describe('parseReportSections', () => {
  it('从 MOCK_REPORT 提取所有段落', () => {
    const sections = parseReportSections(MOCK_REPORT);

    expect(sections['封面']).toContain('标书智能审核报告');
    // Markdown 原文包含 "**投标单位**", 所以搜索纯文本关键内容
    expect(sections['基本信息']).toContain('智网科技股份有限公司');
    expect(sections['审核结论']).toContain('本项目标书整体结构完整');
    expect(sections['问题汇总']).toContain('品牌指定');
    expect(sections['详细列表']).toContain('硬件报价超标');
    expect(sections['审核说明']).toContain('本报告由系统 AI 自动生成');
  });

  it('对自定义 Report 对象同样生效', () => {
    const report: Report = {
      id: 2,
      auditId: 1002,
      docContent: '# 封面\n封面内容\n# 基本信息\n基本信息内容',
      generateTime: '2026-07-24T00:00:00Z',
    };

    const sections = parseReportSections(report);
    expect(sections['封面']).toContain('封面内容');
    expect(sections['基本信息']).toContain('基本信息内容');
    // 其他段落应为空
    expect(sections['审核结论']).toBe('');
    expect(sections['问题汇总']).toBe('');
    expect(sections['详细列表']).toBe('');
    expect(sections['审核说明']).toBe('');
  });

  it('docContent 为空时返回所有空段落', () => {
    const report: Report = {
      id: 3,
      auditId: 1003,
      docContent: '',
      generateTime: '2026-07-24T00:00:00Z',
    };

    const sections = parseReportSections(report);
    REPORT_SECTIONS.forEach((key) => {
      expect(sections[key]).toBe('');
    });
  });
});
