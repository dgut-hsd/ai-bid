import type { AuditIssue } from '../types';

const categories: string[] = ['地域歧视', '品牌指定', '程序违规', '资质排他', '评分倾斜', '需求不清'];
const severities = ['high', 'medium', 'info'] as const;

export const generateMockIssues = (): AuditIssue[] => {
   return Array.from({ length: 20 }).map((_, index) => {
      const severity = severities[index % 3];
      const pageNumber = (index % 15) + 1;

      return {
         issueNo: `ISSUE-${String(index + 1).padStart(4, '0')}`,
         severity,
         category: categories[index % categories.length],
         description: `[测试长文本] 标书第 ${pageNumber} 页发现潜在的${
            severity === 'high'
               ? '严重'
               : severity === 'medium'
               ? '一般'
               : '提示'
         }级风险。此处故意生成极长的问题描述文本，用于测试 Ant Design Table 的单行截断（ellipsis）能力，以及配合 Tooltip 气泡提示框的完整内容展示效果，确保在极小屏幕下也不会导致表格撑破或换行破坏布局。`,
         location: {
            pageNumber,
            sectionName: `第${Math.ceil(pageNumber / 3)}章`,
            context: `该段落位于标书第 ${pageNumber} 页，涉及采购条款的具体约定内容。`,
         },
         suggestion: '请核对最新学校采购管理制度并进行对应章节的修改。',
         reference:
            '《政府采购法》第二十二条、《招标投标法实施条例》第三十四条',
      };
   });
};

export const mockBidDetail = {
   bidName: '2024年校园网络核心交换机采购项目',
   uploadTime: '2024-03-03 14:30:00',
};
