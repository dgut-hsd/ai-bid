import type {
   AuditListItem,
   AuditListQueryParams,
   ProjectItem,
} from '../types';
import type { PageResponse } from '@/api/types';

import { queryOptions } from '@tanstack/react-query';
import dayjs from 'dayjs';

const MOCK_DATA: AuditListItem[] = Array.from({ length: 40 }).map(
   (_, index) => {
      const baseTime = dayjs().subtract(index, 'day');

      return {
         projectId: 1000 + index,
         projectName: `2026年度${
            index % 2 === 0 ? '校园网络扩建' : '教学楼翻新'
         }项目-${index + 1}期`,
         createTime: baseTime.format('YYYY-MM-DD'),
         latestVersion: 1.0 + (index % 3) * 0.1,
         fileCategory: index % 2 === 0 ? '合同' : '标书',
         supplierName:
            ['腾讯云', '阿里云', '华为', '中建三局', '南网科技'][index % 5] +
            '有限公司',
         auditorName: '尼玛',
      };
   }
);

const mockProjectList: ProjectItem[] = Array.from({ length: 8 }).map(
   (_, index) => ({
      id: index + 1,
      fileName: `文档名称_${index + 1}.pdf`,
      filePath: `/path/to/file_${index + 1}.pdf`,
      fileSize: 1024 * (index + 1),
      fileType: 'PDF',
      fileCategory: index % 2 === 0 ? '合同' : '标书',
      bidName: '某某项目招标',
      supplierName: '供应商 A',
      budgetAmount: 100000,
      pageCount: 50,
      parseStatus: 2,
      uploadUserId: 1001,
      uploadTime: '2026-03-17',
      version: index + 1,
      projectId: 100,
      auditorName: '王八',
   })
);

// 2. 模拟异步请求
const fetchAuditList = async (
   params: AuditListQueryParams
): Promise<PageResponse<AuditListItem>> => {
   await new Promise((resolve) => setTimeout(resolve, 600));

   let result = [...MOCK_DATA];

   const {
      page,
      size,
      bidName: projectName,
      fileCategory,
      uploadStartTime,
      uploadEndTime,
   } = params;

   // 1. 过滤：项目名称
   if (projectName) {
      result = result.filter((item) =>
         item.projectName.toLowerCase().includes(projectName.toLowerCase())
      );
   }

   // 2. 过滤：文件类型（筛选值现为存储码 bid/contract，换算成中文标签比对）
   if (fileCategory) {
      const categoryLabel = fileCategory === 'bid' ? '标书' : '合同';
      result = result.filter((item) => item.fileCategory === categoryLabel);
   }

   // 3. 过滤：时间范围
   if (uploadStartTime && uploadEndTime) {
      result = result.filter(
         (item) =>
            item.createTime >= uploadStartTime &&
            item.createTime <= uploadEndTime
      );
   }

   // 分页计算
   const total = result.length;
   const startIndex = (page - 1) * size;
   const paginatedList = result.slice(startIndex, startIndex + size);

   return {
      records: paginatedList,
      total,
   };
};

const fetchProjectVersions = async (
   _projectId: number
): Promise<ProjectItem[]> => {
   await new Promise((resolve) => setTimeout(resolve, 600));

   return mockProjectList;
};

export const mockAuditListOptions = {
   list: (params: AuditListQueryParams) =>
      queryOptions({
         queryKey: ['auditList', params],
         queryFn: () => fetchAuditList(params),
         staleTime: 5 * 60 * 1000,
      }),

   detail: (projectId: number) =>
      queryOptions({
         queryKey: ['projectDetail', projectId],
         queryFn: () => fetchProjectVersions(projectId),
         enabled: !!projectId,
         staleTime: 5 * 60 * 1000,
      }),
};
