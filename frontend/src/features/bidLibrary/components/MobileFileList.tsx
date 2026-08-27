import React, { useState, useEffect, useRef, useCallback } from 'react';
import { Spin, Empty, Tabs } from 'antd';
import { MobileFileCard } from './MobileFileCard';
import type { KnowledgeFile } from '../api/types';

interface MobileFileListProps {
  files: KnowledgeFile[];
  categoryMap: Record<string, string>;
  categoryColorMap: Record<string, string>;
  onView: (file: KnowledgeFile) => void;
  onDownload: (file: KnowledgeFile) => void;
  onEdit: (file: KnowledgeFile) => void;
  onDelete: (file: KnowledgeFile) => void;
  onStatusChange: (file: KnowledgeFile) => void;
  onLoadMore: () => void;
  hasMore: boolean;
  loading: boolean;
  recentlyViewed: KnowledgeFile[];
  favorites: KnowledgeFile[];
  onRefresh: () => void;
}

export const MobileFileList: React.FC<MobileFileListProps> = ({
  files,
  categoryMap,
  categoryColorMap,
  onView,
  onDownload,
  onEdit,
  onDelete,
  onStatusChange,
  onLoadMore,
  hasMore,
  loading,
  recentlyViewed,
  favorites,
  onRefresh,
}) => {
  const [isRefreshing, setIsRefreshing] = useState(false);
  const [refreshText, setRefreshText] = useState('下拉刷新');
  const containerRef = useRef<HTMLDivElement>(null);
  const startY = useRef(0);
  const pullDistance = useRef(0);

  const handleTouchStart = (e: React.TouchEvent) => {
    if (containerRef.current?.scrollTop === 0) {
      startY.current = e.touches[0].clientY;
    }
  };

  const handleTouchMove = (e: React.TouchEvent) => {
    if (containerRef.current?.scrollTop === 0 && startY.current > 0) {
      const currentY = e.touches[0].clientY;
      pullDistance.current = currentY - startY.current;
      
      if (pullDistance.current > 0 && pullDistance.current < 100) {
        setRefreshText('下拉刷新');
        setIsRefreshing(false);
      } else if (pullDistance.current >= 100) {
        setRefreshText('释放刷新');
        setIsRefreshing(true);
      }
    }
  };

  const handleTouchEnd = () => {
    if (isRefreshing && pullDistance.current >= 100) {
      setRefreshText('刷新中...');
      onRefresh();
      setTimeout(() => {
        setIsRefreshing(false);
        setRefreshText('下拉刷新');
        pullDistance.current = 0;
      }, 1000);
    } else {
      setIsRefreshing(false);
      pullDistance.current = 0;
    }
    startY.current = 0;
  };

  const handleScroll = useCallback(() => {
    if (!containerRef.current || loading || !hasMore) return;
    
    const { scrollTop, scrollHeight, clientHeight } = containerRef.current;
    if (scrollHeight - scrollTop - clientHeight < 100) {
      onLoadMore();
    }
  }, [loading, hasMore, onLoadMore]);

  useEffect(() => {
    const container = containerRef.current;
    if (container) {
      container.addEventListener('scroll', handleScroll);
      return () => container.removeEventListener('scroll', handleScroll);
    }
  }, [handleScroll]);

  const tabItems = [
    {
      key: 'all',
      label: '全部文件',
      children: (
        <div className="mobile-file-list-content">
          {files.length > 0 ? (
            files.map((file) => (
              <MobileFileCard
                key={file.id}
                file={file}
                categoryMap={categoryMap}
                categoryColorMap={categoryColorMap}
                onView={onView}
                onDownload={onDownload}
                onEdit={onEdit}
                onDelete={onDelete}
                onStatusChange={onStatusChange}
              />
            ))
          ) : (
            <Empty description="暂无文件" />
          )}
          {loading && (
            <div className="mobile-file-list-loading">
              <Spin />
              <span>加载中...</span>
            </div>
          )}
          {!hasMore && files.length > 0 && (
            <div className="mobile-file-list-end">没有更多了</div>
          )}
        </div>
      ),
    },
    {
      key: 'recent',
      label: '最近访问',
      children: (
        <div className="mobile-file-list-content">
          {recentlyViewed.length > 0 ? (
            recentlyViewed.map((file) => (
              <MobileFileCard
                key={file.id}
                file={file}
                categoryMap={categoryMap}
                categoryColorMap={categoryColorMap}
                onView={onView}
                onDownload={onDownload}
                onEdit={onEdit}
                onDelete={onDelete}
                onStatusChange={onStatusChange}
              />
            ))
          ) : (
            <Empty description="暂无最近访问记录" />
          )}
        </div>
      ),
    },
    {
      key: 'favorites',
      label: '我的收藏',
      children: (
        <div className="mobile-file-list-content">
          {favorites.length > 0 ? (
            favorites.map((file) => (
              <MobileFileCard
                key={file.id}
                file={file}
                categoryMap={categoryMap}
                categoryColorMap={categoryColorMap}
                onView={onView}
                onDownload={onDownload}
                onEdit={onEdit}
                onDelete={onDelete}
                onStatusChange={onStatusChange}
              />
            ))
          ) : (
            <Empty description="暂无收藏文件" />
          )}
        </div>
      ),
    },
  ];

  return (
    <div
      ref={containerRef}
      className="mobile-file-list"
      onTouchStart={handleTouchStart}
      onTouchMove={handleTouchMove}
      onTouchEnd={handleTouchEnd}
    >
      <div className={`mobile-file-list-pull ${isRefreshing ? 'active' : ''}`}>
        <span>{refreshText}</span>
      </div>
      <Tabs
        defaultActiveKey="all"
        items={tabItems}
        className="mobile-file-list-tabs"
      />
    </div>
  );
};
