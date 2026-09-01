import React from 'react';
import { Skeleton } from 'antd';

interface LoadingProps {
   loading: boolean;
   children?: React.ReactNode;
   description?: string;
   fullScreen?: boolean;
}

/**
 * 通用加载占位：以骨架屏替代转圈 Spin。
 * - loading=true 时渲染骨架屏（标题条 + 内容块）
 * - loading=false 时直接渲染 children（保留作为包裹容器的用法）
 */
export const Loading: React.FC<LoadingProps> = ({
   loading,
   children,
   fullScreen = false,
}) => {
   if (!loading) {
      return <>{children}</>;
   }

   return (
      <div
         style={{
            padding: fullScreen ? 24 : 8,
            minHeight: fullScreen ? 320 : undefined,
         }}
      >
         <Skeleton
            active
            title={{ width: 180 }}
            paragraph={{ rows: 1 }}
         />
         <Skeleton
            active
            paragraph={{ rows: 6 }}
            style={{ marginTop: 24 }}
         />
      </div>
   );
};