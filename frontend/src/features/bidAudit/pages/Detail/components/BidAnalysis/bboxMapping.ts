import type { BBoxData } from '../../components/PDFPreview/PdfPreview';

/**
 * 将 Java 代理端点 `/api/audit-tasks/{taskId}/blocks` 返回的 block BBox 列表
 * 映射为 PdfPreview 高亮需要的数据。
 *
 * 兼容两套字段命名（后端分两跳，命名可能不一致）：
 * - Rust→Java 用 SNAKE_CASE 反序列化，Java 再以默认 camelCase 序列化回前端（`pageWidth`）；
 * - 历史上直接透传 snake_case 时输出 `page_width`。
 * 两套都读，避免 `pageWidth` 恒回落 595 造成非 A4 页面错位。
 */
export function mapBBoxEntries(list: unknown): BBoxData[] {
  if (!Array.isArray(list)) return [];
  return list.map((item: any): BBoxData => ({
    x0: item?.bbox?.x0 ?? 0,
    top: item?.bbox?.top ?? 0,
    x1: item?.bbox?.x1 ?? 0,
    bottom: item?.bbox?.bottom ?? 0,
    pageWidth: item?.pageWidth ?? item?.page_width ?? 595,
    // 后端 page 为 0-based → 前端 1-based（与 PdfPreview data-page-num 对齐）
    page: (item?.page ?? 0) + 1,
  }));
}