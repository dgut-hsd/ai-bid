import { saveAs } from 'file-saver';
import { asBlob } from 'html-docx-js-typescript';
import type { Report } from './types';

export const MOCK_REPORT: Report = {
   id: 1,
   auditId: 1001,
   docContent: `
# 封面
## 标书智能审核报告
**项目名称**：2026年度数字校园智慧基座建设项目  
**审核日期**：2026-03-06  
**审核版本**：V1.0.0  

---
# 基本信息
- **投标单位**：智网科技股份有限公司
- **标书总页数**：156页
- **检测耗时**：4.2秒

---
# 审核结论
本项目标书整体结构完整，但在**品牌指定**和**程序违规**维度存在关键阻断性问题。  
**结论建议**：修改后复审。

---
# 问题汇总
| 风险类型 | 严重风险 (Severe) | 警告风险 (Warning) | 提示建议 (Info) |
| :--- | :---: | :---: | :---: |
| 品牌指定 | 2 | 1 | 0 |
| 需求不清 | 0 | 5 | 12 |
| 程序违规 | 1 | 0 | 0 |

---
# 详细列表
### 1. [严重] 硬件报价超标 (第12页)
* **风险类型**：品牌指定
* **问题描述**：核心交换机报价超过发改委指导价上限 15%。
* **修改建议**：请参照《2026政务采购指导目录》调整单价，或提供特殊定价说明函。

### 2. [严重] 缺席反商业贿赂条款 (第145页)
* **风险类型**：程序违规
* **问题描述**：法务合规性扫描未发现标准的《反商业贿赂承诺书》。
* **修改建议**：请在附件章节补充标准模板的承诺书并加盖公章。

---
# 审核说明
1. 本报告由系统 AI 自动生成，仅供辅助决策。
2. 最终定标权归评标委员会所有。
`,
   generateTime: '2026-03-06T10:30:00',
};

export const REPORT_SECTIONS = [
   '封面',
   '基本信息',
   '审核结论',
   '问题汇总',
   '详细列表',
   '审核说明',
] as const;

export type SectionTitle = (typeof REPORT_SECTIONS)[number];

export function parseMarkdownToSections(
   fullMd: string
): Record<SectionTitle, string> {
   const sections = {} as Record<SectionTitle, string>;

   REPORT_SECTIONS.forEach((key) => (sections[key] = ''));

   const validTitlesPattern = REPORT_SECTIONS.join('|');
   const regex = new RegExp(
      `(?:^|\\n)#\\s+(${validTitlesPattern})\\s*(?=\\n|$)`,
      'g'
   );

   let match;
   let lastValidTitle: SectionTitle | null = null;
   let lastMatchStartIndex = 0;

   while ((match = regex.exec(fullMd)) !== null) {
      const matchedTitle = match[1] as SectionTitle;

      if (lastValidTitle) {
         sections[lastValidTitle] = fullMd
            .substring(lastMatchStartIndex, match.index)
            .trim();
      }

      lastValidTitle = matchedTitle;
      lastMatchStartIndex = match.index;
   }

   if (lastValidTitle) {
      sections[lastValidTitle] = fullMd.substring(lastMatchStartIndex).trim();
   }

   const hasMatchedSections = REPORT_SECTIONS.some(
      (key) => sections[key]?.trim().length > 0
   );
   if (hasMatchedSections) {
      return sections;
   }

   // 兼容后端当前生成格式：# 标书审核报告 + ## 一、标书基本信息/二、审核结论...
   const normalized = fullMd || '';
   const headingRegex = /(?:^|\n)##\s*([一二三四五六七八九十]+)、([^\n]+)\s*/g;
   const matches: Array<{ index: number; rawTitle: string; heading: string }> = [];
   let m: RegExpExecArray | null;
   while ((m = headingRegex.exec(normalized)) !== null) {
      matches.push({
         index: m.index,
         rawTitle: m[0],
         heading: `${m[1]}、${m[2]}`.trim(),
      });
   }

   if (matches.length === 0) {
      return sections;
   }

   const headingToSection = (heading: string): SectionTitle | null => {
      if (heading.includes('标书基本信息')) return '基本信息';
      if (heading.includes('审核结论')) return '审核结论';
      if (heading.includes('问题汇总')) return '问题汇总';
      if (heading.includes('详细问题列表')) return '详细列表';
      if (heading.includes('审核说明')) return '审核说明';
      return null;
   };

   const coverContent = normalized.slice(0, matches[0].index).trim();
   if (coverContent) {
      sections.封面 = coverContent;
   }

   for (let i = 0; i < matches.length; i++) {
      const start = matches[i].index;
      const end = i + 1 < matches.length ? matches[i + 1].index : normalized.length;
      const block = normalized.slice(start, end).trim();
      const sectionKey = headingToSection(matches[i].heading);
      if (sectionKey && block) {
         sections[sectionKey] = block;
      }
   }

   return sections;
}

export function parseReportSections(
   report: Report
): Record<SectionTitle, string> {
   return parseMarkdownToSections(report.docContent);
}

/**
 * 从 Markdown 报告内容中提取项目名称，用于推导默认导出文件名。
 * 兼容两种格式：
 *   - 后端生成："**项目名称：** 2026年度..."（冒号在星号内）
 *   - Mock 数据："**项目名称**：2026年度..."（冒号在星号外）
 */
export function extractBidName(docContent: string): string | null {
   if (!docContent) return null;
   const match = docContent.match(/项目名称\**\s*[：:]\s*\**\s*([^\n\r]+)/);
   if (!match) return null;
   const value = match[1].trim().replace(/\*+$/, '').trim();
   return value || null;
}

export async function generateWordDocument(
   htmlContent: string,
   fileName: string
): Promise<void> {
   try {
      const wrappedHtml = `
       <!DOCTYPE html>
       <html>
         <head>
           <meta charset="UTF-8">
           <style>
             body { font-family: "Microsoft YaHei", sans-serif; font-size: 11pt; }
             h1 { font-size: 18pt; text-align: center; margin-bottom: 24pt; }
             h2 { font-size: 14pt; margin-top: 18pt; }
             table { border-collapse: collapse; width: 100%; margin: 12pt 0; }
             th, td { border: 1px solid #000000; padding: 6pt; text-align: left; }
             th { background-color: #f2f2f2; font-weight: bold; }
           </style>
         </head>
         <body>
           ${htmlContent}
         </body>
       </html>
     `;

      const documentOptions = {
         orientation: 'portrait' as const,
         margins: { top: 1440, right: 1440, bottom: 1440, left: 1440 },
      };

      const documentBlob = await asBlob(wrappedHtml, documentOptions);

      saveAs(documentBlob as Blob, fileName);
   } catch (error) {
      console.error('Word 导出失败:', error);
      throw new Error('文档生成失败，请检查浏览器内存或内容格式。');
   }
}
