import { QueryClient } from '@tanstack/react-query';

export const queryClient = new QueryClient({
   defaultOptions: {
      queries: {
         retry: 1,
         refetchOnWindowFocus: false,
         staleTime: 1000 * 60 * 5,
         // 缓存保留 10 分钟（默认 5 分钟）：来回切换页面时列表数据更少被重新请求
         gcTime: 1000 * 60 * 10,
      },
   },
});

// 兼容 React Router v7
(queryClient as any).defaultQueryOptions = (options: any) => ({
   ...options,
   ...queryClient.getDefaultOptions(),
});

export default queryClient;
