import React from 'react';
import { Spin, Empty } from 'antd';
import { Document, Page, pdfjs } from 'react-pdf';
import { useStyles } from '../../style';
import { usePdfFlow } from '../../hooks/usePdfFlow';
import { PdfToolbar } from './PdfToolbar';
import 'react-pdf/dist/Page/TextLayer.css';
import 'react-pdf/dist/Page/AnnotationLayer.css';

import PdfWorker from 'pdfjs-dist/build/pdf.worker.min.mjs?url';

pdfjs.GlobalWorkerOptions.workerSrc = PdfWorker;

interface PdfPreviewProps {
   fileUrl: string;
   fileType: string;
   isComplete: boolean;
}

type HighlightStatus = 'idle' | 'exact_hit' | 'token_fallback_hit' | 'miss';

/** BBox 坐标数据（来自后端 API，PDF points 坐标系） */
export type BBoxData = {
  x0: number;
  top: number;
  x1: number;
  bottom: number;
  pageWidth: number;
  page?: number; // ← 新增：该 BBox 属于哪一页
};

type HighlightBox = {
   left: number;
   top: number;
   width: number;
   height: number;
   primary: boolean;
};

type CharBox = {
   left: number;
   top: number;
   width: number;
   height: number;
};

type PageTextIndex = {
   compactText: string;
   charBoxes: CharBox[];
};

export interface PdfPreviewRef {
   jumpToPage: (page: number, highlightText?: string, fallbackTokens?: string[]) => void;
   /** BBox-based 精确高亮：直接按坐标渲染高亮矩形（PDF points → DOM 像素） */
   highlightBboxes: (page: number, bboxes: BBoxData[]) => void;
}

const normalizeHighlightText = (value?: string): string => {
   const raw = String(value || '')
      .replace(/\s+/g, ' ')
      .trim();
   if (!raw) return '';
   const candidate = raw.length > 80 ? raw.slice(0, 80) : raw;
   return candidate.replace(/[，,。；;：:!?！？、]/g, ' ').replace(/\s+/g, ' ').trim();
};

const buildHighlightTokens = (value: string): string[] => {
   const normalized = String(value || '')
      .replace(/[\r\n\t]+/g, ' ')
      .replace(/[，,。；;：:!?！？、（）()\[\]【】"“”'‘’]/g, ' ')
      .replace(/\s+/g, ' ')
      .trim();
   const rough = normalized
      .split(/\s+/)
      .map((s) => s.trim())
      .filter((s) => s.length >= 2 && s.length <= 40);
   const refined: string[] = [];
   for (const token of rough) {
      if (/[\u4e00-\u9fff]/.test(token) && token.length > 6) {
         for (let n = 4; n >= 2; n--) {
            for (let i = 0; i + n <= token.length; i += Math.max(1, Math.floor(n / 2))) {
               refined.push(token.slice(i, i + n));
            }
         }
      } else {
         refined.push(token);
      }
   }
   const unique = Array.from(new Set(refined));
   const scored = unique
      .map((token) => {
         const hasNumber = /\d/.test(token) ? 1 : 0;
         const hasSymbol = /[@%:：\-]/.test(token) ? 1 : 0;
         return { token, score: hasNumber * 4 + hasSymbol * 2 + Math.min(token.length, 10) };
      })
      .sort((a, b) => b.score - a.score);
   return scored.map((item) => item.token).slice(0, 10);
};

const normalizeTokenList = (tokens?: string[]): string[] =>
   Array.from(
      new Set(
         (tokens || [])
            .map((item) => String(item || '').trim())
            .filter((item) => item.length >= 2 && item.length <= 40)
      )
   ).slice(0, 8);

const compactForMatch = (value: string): string =>
   String(value || '')
      .replace(/[\s，,。；;：:!?！？、（）()\[\]【】"“”'‘’]/g, '')
      .trim()
      .toLowerCase();

const buildHighlightCandidates = (query: string, tokens: string[]): string[] => {
   const candidates = Array.from(
      new Set(
         [query, ...(tokens || [])]
            .map((item) => String(item || '').trim())
            .filter((item) => item.length >= 2)
      )
   );
   return candidates.sort((a, b) => b.length - a.length).slice(0, 10);
};

const findAllCompactPositions = (text: string, keyword: string, limit: number = 8): number[] => {
   const out: number[] = [];
   if (!text || !keyword) return out;
   let from = 0;
   while (from < text.length && out.length < limit) {
      const at = text.indexOf(keyword, from);
      if (at < 0) break;
      out.push(at);
      from = at + Math.max(1, keyword.length);
   }
   return out;
};

const mergeCharBoxes = (charBoxes: CharBox[], tolerancePx: number = 2): CharBox[] => {
   if (!charBoxes.length) return [];
   const sorted = [...charBoxes].sort((a, b) => {
      if (Math.abs(a.top - b.top) > tolerancePx) {
         return a.top - b.top;
      }
      return a.left - b.left;
   });
   const merged: CharBox[] = [];
   for (const box of sorted) {
      const last = merged[merged.length - 1];
      if (!last) {
         merged.push({ ...box });
         continue;
      }
      const sameLine = Math.abs(last.top - box.top) <= tolerancePx;
      const touching = box.left <= last.left + last.width + 1.5;
      if (sameLine && touching) {
         const right = Math.max(last.left + last.width, box.left + box.width);
         last.left = Math.min(last.left, box.left);
         last.top = Math.min(last.top, box.top);
         last.height = Math.max(last.height, box.height);
         last.width = right - last.left;
      } else {
         merged.push({ ...box });
      }
   }
   return merged;
};

const buildPageCandidates = (value: string): string[] => {
   const normalized = String(value || '')
      .replace(/[\r\n\t]+/g, ' ')
      .trim();
   const parts = normalized
      .split(/[，,。；;：:!?！？、（）()\[\]【】]/)
      .map((s) => s.trim())
      .filter((s) => s.length >= 4 && s.length <= 40);
   const tokens = buildHighlightTokens(normalized);
   const set = new Set<string>([normalized, ...parts, ...tokens]);
   return Array.from(set)
      .filter((s) => s.length >= 4)
      .sort((a, b) => b.length - a.length)
      .slice(0, 12);
};

const buildPageProbeOrder = (preferredPage: number, totalPages: number): number[] => {
   const out: number[] = [];
   const seen = new Set<number>();
   const push = (p: number) => {
      if (p >= 1 && p <= totalPages && !seen.has(p)) {
         seen.add(p);
         out.push(p);
      }
   };
   push(preferredPage);
   for (let d = 1; d <= 3; d++) {
      push(preferredPage - d);
      push(preferredPage + d);
   }
   for (let p = 1; p <= totalPages; p++) {
      push(p);
   }
   return out;
};

const PdfPreview = React.forwardRef<PdfPreviewRef, PdfPreviewProps>(({
   fileUrl,
   fileType,
   isComplete,
}, ref) => {
   const { styles } = useStyles();
   const {
      containerRef,
      scale,
      numPages,
      setNumPages,
      currentPage,

      zoomIn,
      zoomOut,
      resetZoom,

      jumpToPage,
   } = usePdfFlow(isComplete);

   const [previewFailed, setPreviewFailed] = React.useState(false);
   const [highlightText, setHighlightText] = React.useState('');
   const [, setHighlightStatus] = React.useState<HighlightStatus>('idle');
   const [highlightBoxesByPage, setHighlightBoxesByPage] = React.useState<Record<number, HighlightBox[]>>({});
   // ── PDF 懒渲染：只挂载可视区附近及被标记的页，避免 120 页一次性全量渲染 ──
   const [renderedPages, setRenderedPages] = React.useState<Set<number>>(() => new Set([1]));

   const markPageRendered = React.useCallback((page: number) => {
      setRenderedPages((prev) => {
         if (!page || prev.has(page)) return prev;
         const next = new Set(prev);
         next.add(page);
         return next;
      });
   }, []);
   const pdfDocRef = React.useRef<any | null>(null);
   const pageDimRef = React.useRef<Record<number, { width: number; height: number }>>({}); //缓存每页"原生 PDF 点尺寸"，用于高亮坐标换算
   const pageTextIndexCacheRef = React.useRef<Record<string, PageTextIndex>>({});
   const highlightQueryRef = React.useRef('');
   const highlightTokensRef = React.useRef<string[]>([]);
   const fallbackTokensRef = React.useRef<string[]>([]);
   const pendingSecondaryMatchRef = React.useRef<string>('');
   React.useEffect(() => {
      const normalized = normalizeHighlightText(highlightText);
      highlightQueryRef.current = normalized;
      const autoTokens = buildHighlightTokens(normalized);
      const merged = Array.from(new Set([...fallbackTokensRef.current, ...autoTokens]));
      highlightTokensRef.current = merged.slice(0, 10);
   }, [highlightText]);

   React.useEffect(() => {
      pageTextIndexCacheRef.current = {};
   }, [scale, fileUrl]);

   // 纯 overlay 高亮：不再直接改 react-pdf 文本层 DOM，避免与 React 重渲染（SSE 实时进度）冲突产生 insertBefore
   const clearSpanHighlights = React.useCallback((page: number) => {
      if (!page) return;
      setHighlightBoxesByPage((prev) => {
         if (!prev[page]) return prev;
         const next = { ...prev };
         delete next[page];
         return next;
      });
   }, []);

   const getPageTextIndex = React.useCallback(
      async (page: number): Promise<PageTextIndex | null> => {
         const doc = pdfDocRef.current;
         if (!doc || !page || page < 1) return null;
         const cacheKey = `${page}@${scale.toFixed(3)}`;
         const cached = pageTextIndexCacheRef.current[cacheKey];
         if (cached) return cached;
         try {
            const pdfPage = await doc.getPage(page);
            const baseViewport = pdfPage.getViewport({ scale: 1 });
            const renderWidth = 800 * scale;
            const renderScale = renderWidth / Math.max(baseViewport.width || 1, 1);
            const textContent = await pdfPage.getTextContent();
            const compactChars: string[] = [];
            const charBoxes: CharBox[] = [];
            const items = Array.isArray((textContent as any)?.items)
               ? ((textContent as any).items as any[])
               : [];
            items.forEach((item) => {
               const raw = String(item?.str || '');
               const compact = compactForMatch(raw);
               if (!compact) return;
               const t = Array.isArray(item?.transform) ? item.transform : [1, 0, 0, 1, 0, 0];
               const x = Number(t[4] || 0) * renderScale;
               const yBase = (Number(baseViewport.height || 0) - Number(t[5] || 0)) * renderScale;
               const h = Math.max(Math.abs(Number(t[3] || 0) * renderScale), 9);
               const wRaw = Number(item?.width || 0) * renderScale;
               const w = Math.max(wRaw, Math.max(6, compact.length * 6));
               const top = Math.max(0, yBase - h);
               const perChar = w / compact.length;
               for (let i = 0; i < compact.length; i++) {
                  compactChars.push(compact[i]);
                  charBoxes.push({
                     left: x + i * perChar,
                     top,
                     width: Math.max(2, perChar),
                     height: h,
                  });
               }
            });
            if (!compactChars.length || !charBoxes.length) {
               return null;
            }
            const built: PageTextIndex = {
               compactText: compactChars.join(''),
               charBoxes,
            };
            pageTextIndexCacheRef.current[cacheKey] = built;
            return built;
         } catch (err) {
            console.warn('[pdf-highlight] build page text index failed:', err);
            return null;
         }
      },
      [scale]
   );

   const applyPdfJsHighlights = React.useCallback(
      async (
         page: number,
         query: string,
         tokens: string[],
         options?: { silent?: boolean; maxMatches?: number }
      ): Promise<boolean> => {
         const silent = !!options?.silent;
         const maxMatches = Math.max(1, Math.min(options?.maxMatches ?? 6, 12));
         const index = await getPageTextIndex(page);
         if (!index) {
            return false;
         }
         const { compactText, charBoxes } = index;
         if (!compactText || !charBoxes.length) return false;
         const exactCompact = compactForMatch(query);
         let matchStatus: HighlightStatus = 'miss';
         let selectedKeyword = '';
         // 多候选匹配：每条匹配记录含 compact 文本起始位置和长度
         // （不同候选 keyword 长度不同，不能再用全局 selectedCompact）
         const matchInfos: Array<{ start: number; length: number }> = [];
         if (exactCompact.length >= 2) {
            const indexes = findAllCompactPositions(compactText, exactCompact, maxMatches);
            if (indexes.length) {
               matchStatus = 'exact_hit';
               selectedKeyword = query;
               for (const idx of indexes) {
                  matchInfos.push({ start: idx, length: exactCompact.length });
               }
            }
         }
         // ★ 子串回退：把 source_quote 按标点拆成多段，每段独立匹配，
         // 合并所有不重叠的命中 → 长 source_quote 不会只亮前一两句。
         if (!matchInfos.length) {
            const pageCandidates = buildPageCandidates(query)
               .map((item) => ({ raw: item, compact: compactForMatch(item) }))
               .filter((item) => item.compact.length >= 4);
            const seenRanges = new Set<number>();
            for (const candidate of pageCandidates) {
               if (matchInfos.length >= maxMatches) break;
               const indexes = findAllCompactPositions(
                  compactText,
                  candidate.compact,
                  maxMatches - matchInfos.length
               );
               for (const idx of indexes) {
                  // 跳过与已有匹配重叠的命中
                  let overlap = false;
                  for (let j = idx; j < idx + candidate.compact.length; j++) {
                     if (seenRanges.has(j)) {
                        overlap = true;
                        break;
                     }
                  }
                  if (overlap) continue;
                  matchInfos.push({ start: idx, length: candidate.compact.length });
                  for (let j = idx; j < idx + candidate.compact.length; j++) {
                     seenRanges.add(j);
                  }
               }
               if (matchInfos.length > 0 && !selectedKeyword) {
                  matchStatus = 'token_fallback_hit';
                  selectedKeyword = query;
               }
            }
         }
         // 子串回退也没结果时降级到 token 粒度的兜底
         if (!matchInfos.length) {
            const tokenCandidates = buildHighlightCandidates('', tokens)
               .map((item) => ({ raw: item, compact: compactForMatch(item) }))
               .filter((item) => item.compact.length >= 2)
               .sort((a, b) => b.compact.length - a.compact.length);
            for (const candidate of tokenCandidates) {
               const indexes = findAllCompactPositions(compactText, candidate.compact, maxMatches);
               if (!indexes.length) continue;
               matchStatus = 'token_fallback_hit';
               selectedKeyword = candidate.raw;
               for (const idx of indexes) {
                  matchInfos.push({ start: idx, length: candidate.compact.length });
               }
               break;
            }
         }
         if (!matchInfos.length || !selectedKeyword) {
            if (!silent) {
               setHighlightStatus('miss');
               console.info('[pdf-highlight] status=miss page=%s query=%s', page, query);
            }
            return false;
         }
         const boxes: HighlightBox[] = [];
         matchInfos.forEach(({ start, length }, occIdx) => {
            const perOcc: CharBox[] = [];
            for (let i = start; i < start + length && i < charBoxes.length; i++) {
               perOcc.push(charBoxes[i]);
            }
            const merged = mergeCharBoxes(perOcc);
            merged.forEach((b) =>
               boxes.push({
                  ...b,
                  primary: occIdx === 0,
               })
            );
         });
         if (!boxes.length) return false;
         setHighlightBoxesByPage((prev) => ({
            ...prev,
            [page]: boxes,
         }));
         if (!silent) {
            setHighlightStatus(matchStatus);
            console.info(
               '[pdf-highlight] status=%s page=%s keyword=%s matches=%s engine=pdfjs',
               matchStatus,
               page,
               selectedKeyword,
               matchInfos.length
            );
         }
         return true;
      },
      [getPageTextIndex]
   );

   const applySpanHighlights = React.useCallback(
      (
         page: number,
         query: string,
         tokens: string[],
         retry = 8,
         options?: { silent?: boolean; maxMatches?: number }
      ): boolean => {
         if (!containerRef.current || !page) return false;
         const silent = !!options?.silent;
         const maxMatches = Math.max(1, Math.min(options?.maxMatches ?? 6, 12));
         const layer = containerRef.current.querySelector(
            `[data-page-num="${page}"] .react-pdf__Page__textContent`
         ) as HTMLElement | null;
         if (!layer) {
            if (retry > 0) {
               window.setTimeout(
                  () => applySpanHighlights(page, query, tokens, retry - 1, options),
                  100
               );
            }
            return false;
         }
         const pageRoot = layer.closest('[data-page-num]') as HTMLElement | null;
         if (!pageRoot) return false;
         const spans = Array.from(layer.querySelectorAll('span')) as HTMLElement[];
         if (!spans.length) {
            if (retry > 0) {
               window.setTimeout(
                  () => applySpanHighlights(page, query, tokens, retry - 1, options),
                  100
               );
            }
            return false;
         }
         clearSpanHighlights(page);
         const compactChars: string[] = [];
         const charToSpan: number[] = [];
         spans.forEach((span, spanIndex) => {
            const compact = compactForMatch(String(span.textContent || ''));
            if (!compact) return;
            for (let i = 0; i < compact.length; i++) {
               compactChars.push(compact[i]);
               charToSpan.push(spanIndex);
            }
         });
         const compactText = compactChars.join('');
         if (!compactText) return false;
         const exactCompact = compactForMatch(query);
         let matchStatus: HighlightStatus = 'miss';
         let selectedKeyword = '';
         let selectedCompact = '';
         let selectedIndexes: number[] = [];
         if (exactCompact.length >= 2) {
            const indexes = findAllCompactPositions(compactText, exactCompact, maxMatches);
            if (indexes.length) {
               matchStatus = 'exact_hit';
               selectedKeyword = query;
               selectedCompact = exactCompact;
               selectedIndexes = indexes;
            }
         }
         if (!selectedIndexes.length) {
            const tokenCandidates = buildHighlightCandidates('', tokens)
               .map((item) => ({ raw: item, compact: compactForMatch(item) }))
               .filter((item) => item.compact.length >= 2)
               .sort((a, b) => b.compact.length - a.compact.length);
            for (const candidate of tokenCandidates) {
               const indexes = findAllCompactPositions(compactText, candidate.compact, maxMatches);
               if (!indexes.length) continue;
               matchStatus = 'token_fallback_hit';
               selectedKeyword = candidate.raw;
               selectedCompact = candidate.compact;
               selectedIndexes = indexes;
               break;
            }
         }
         if (!selectedIndexes.length || !selectedCompact) {
            if (!silent) {
               setHighlightStatus('miss');
               console.info('[pdf-highlight] status=miss page=%s query=%s', page, query);
            }
            return false;
         }
         const pageRect = pageRoot.getBoundingClientRect();
         const allBoxes: HighlightBox[] = [];
         selectedIndexes.forEach((start, occIdx) => {
            const spanIndexes = new Set<number>();
            for (
               let i = start;
               i < start + selectedCompact.length && i < charToSpan.length;
               i++
            ) {
               spanIndexes.add(charToSpan[i]);
            }
            const isPrimary = occIdx === 0;
            spanIndexes.forEach((spanIndex) => {
               const span = spans[spanIndex];
               if (!span) return;
               const rect = span.getBoundingClientRect();
               if (rect.width <= 0 || rect.height <= 0) return;
               allBoxes.push({
                  left: rect.left - pageRect.left,
                  top: rect.top - pageRect.top,
                  width: rect.width,
                  height: rect.height,
                  primary: isPrimary,
               });
            });
         });
         if (!allBoxes.length) {
            if (!silent) {
               setHighlightStatus('miss');
               console.info('[pdf-highlight] status=miss page=%s query=%s', page, query);
            }
            return false;
         }
         setHighlightBoxesByPage((prev) => ({
            ...prev,
            [page]: allBoxes,
         }));
         if (!silent) {
            setHighlightStatus(matchStatus);
            console.info(
               '[pdf-highlight] status=%s page=%s keyword=%s matches=%s',
               matchStatus,
               page,
               selectedKeyword,
               selectedIndexes.length
            );
         }
         return true;
      },
      [containerRef, clearSpanHighlights]
   );

   const scrollFirstHitIntoView = React.useCallback(
      (page: number, retry: number = 6) => {
         if (!containerRef.current || !page) return;
         const container = containerRef.current;
         const overlayHit = container.querySelector(
            `[data-page-num="${page}"] [data-overlay-hit="1"]`
         ) as HTMLElement | null;
         if (overlayHit) {
            overlayHit.scrollIntoView({
               behavior: 'smooth',
               block: 'center',
               inline: 'nearest',
            });
            return;
         }
         const pageTextLayer = container.querySelector(
            `[data-page-num="${page}"] .react-pdf__Page__textContent`
         ) as HTMLElement | null;
         const hit = pageTextLayer?.querySelector('span[data-pdf-hit="1"]') as HTMLElement | null;
         if (!hit) {
            if (retry > 0) {
               window.setTimeout(() => scrollFirstHitIntoView(page, retry - 1), 120);
            }
            return;
         }
         hit.scrollIntoView({
            behavior: 'smooth',
            block: 'center',
            inline: 'nearest',
         });
      },
      [containerRef]
   );

   const renderPages = () => {
      if (!numPages) return null;

      const pageWidth = 800 * scale;

      return Array.from(new Array(numPages), (_, index) => {
         const pageNum = index + 1;
         const pageKey = `page_${pageNum}`;
         return (
            <div
               key={pageKey}
               className={styles.pageItem}
               data-page-num={pageNum}
               style={{
                  width: pageWidth,
                  margin: '10px auto',
                  minHeight: pageWidth * 1.414,
                  position: 'relative',
               }}
            >
               {renderedPages.has(pageNum) && (
                  <>
                     <Page
                        pageNumber={pageNum}
                        width={pageWidth}
                        renderAnnotationLayer={false}
                        renderTextLayer
                        devicePixelRatio={1.5}
                     />
                     {(highlightBoxesByPage[pageNum] || []).length > 0 && (
                        <div
                           style={{
                              position: 'absolute',
                              inset: 0,
                              pointerEvents: 'none',
                              zIndex: 3,
                           }}
                        >
                           {(highlightBoxesByPage[pageNum] || []).map((box, idx) => (
                              <div
                                 key={`${pageNum}_${idx}`}
                                 data-overlay-hit={box.primary ? '1' : '2'}
                                 style={{
                                    position: 'absolute',
                                    left: `${box.left}px`,
                                    top: `${box.top}px`,
                                    width: `${box.width}px`,
                                    height: `${box.height}px`,
                                    background: box.primary
                                       ? 'rgba(255, 230, 0, 0.46)'
                                       : 'rgba(255, 230, 0, 0.22)',
                                    borderRadius: 2,
                                 }}
                              />
                           ))}
                        </div>
                     )}
                  </>
               )}
            </div>
         );
      });
   };

   const applySecondaryPageMatch = React.useCallback((page: number, rawQuery?: string) => {
      if (!containerRef.current || !page || !rawQuery) return;
      const query = normalizeHighlightText(rawQuery);
      if (!query) return;
      const run = (retry: number) => {
         const container = containerRef.current;
         if (!container) return;
         const pageEl = container.querySelector(
            `[data-page-num="${page}"] .react-pdf__Page__textContent`
         ) as HTMLElement | null;
         if (!pageEl) {
            if (retry > 0) {
               window.setTimeout(() => run(retry - 1), 120);
            }
            return;
         }
         const pageText = Array.from(pageEl.querySelectorAll('span'))
            .map((el) => String(el.textContent || '').trim())
            .filter(Boolean)
            .join('');
         const compactPage = compactForMatch(pageText);
         if (!compactPage) return;
         const candidates = buildPageCandidates(query);
         let best = '';
         let bestScore = -1;
         for (const candidate of candidates) {
            const compactCandidate = compactForMatch(candidate);
            if (compactCandidate.length < 4) continue;
            if (compactPage.includes(compactCandidate)) {
               const score = compactCandidate.length;
               if (score > bestScore) {
                  bestScore = score;
                  best = candidate;
               }
            }
         }
         if (best && best !== highlightQueryRef.current) {
            setHighlightText(best);
            window.setTimeout(async () => {
               const ok = await applyPdfJsHighlights(page, best, fallbackTokensRef.current, {
                  silent: false,
                  maxMatches: 6,
               });
               if (!ok) {
                  applySpanHighlights(page, best, fallbackTokensRef.current, 6);
               }
               scrollFirstHitIntoView(page);
            }, 90);
         }
      };
      run(6);
   }, [containerRef, scrollFirstHitIntoView, applySpanHighlights, applyPdfJsHighlights]);

   const authToken =
      typeof window === 'undefined'
         ? ''
         : window.localStorage.getItem('token') ||
           window.sessionStorage.getItem('token') ||
           '';

   const documentFile = React.useMemo(
      () =>
         authToken && authToken.trim().length > 0
            ? {
                 url: fileUrl,
                 httpHeaders: {
                    Authorization: `Bearer ${authToken}`,
                 },
              }
            : fileUrl,
      [fileUrl, authToken]
   );

   const lowerType = (fileType || '').toLowerCase().trim();
   const previewableTypes = new Set([
      'pdf',
      'word',
      'doc',
      'docx',
      'application/pdf',
      'application/msword',
      'application/vnd.openxmlformats-officedocument.wordprocessingml.document',
   ]);
   const isPdf = !previewFailed && previewableTypes.has(lowerType);

   // 滚动时懒加载页：仅挂载进入可视区（含上下各一屏预渲染）的页，挂载后保持不清除
   React.useEffect(() => {
      const container = containerRef.current;
      if (!container || !numPages) return;
      const observer = new IntersectionObserver(
         (entries) => {
            entries.forEach((entry) => {
               if (entry.isIntersecting) {
                  const pageNum = Number(
                     (entry.target as HTMLElement).getAttribute('data-page-num') || 0
                  );
                  if (pageNum) markPageRendered(pageNum);
               }
            });
         },
         { root: container, rootMargin: '100% 0px 100% 0px' }
      );
      container.querySelectorAll('[data-page-num]').forEach((el) => observer.observe(el));
      return () => observer.disconnect();
   }, [numPages, isPdf, containerRef, markPageRendered]);

   React.useImperativeHandle(
      ref,
      () => ({
         jumpToPage: (page: number, text?: string, fallbackTokens?: string[]) => {
            const normalized = normalizeHighlightText(text);
            fallbackTokensRef.current = normalizeTokenList(fallbackTokens);
            setHighlightStatus('idle');
            setHighlightBoxesByPage({});
            setHighlightText(normalized);
            markPageRendered(page);
            jumpToPage(page);
            const marker = `${page}|${normalized}`;
            if (pendingSecondaryMatchRef.current !== marker) {
               pendingSecondaryMatchRef.current = marker;
               window.setTimeout(() => {
                  applySecondaryPageMatch(page, normalized);
               }, 80);
            }
            window.setTimeout(async () => {
               const ok = await applyPdfJsHighlights(page, normalized, fallbackTokensRef.current, {
                  silent: false,
                  maxMatches: 8,
               });
               if (!ok) {
                  let locatedPage = 0;
                  if (numPages > 1) {
                     const probeOrder = buildPageProbeOrder(page, numPages);
                     for (const probePage of probeOrder) {
                        if (probePage === page) {
                           continue;
                        }
                        const probeHit = await applyPdfJsHighlights(
                           probePage,
                           normalized,
                           fallbackTokensRef.current,
                           { silent: true, maxMatches: 4 }
                        );
                        if (probeHit) {
                           locatedPage = probePage;
                           break;
                        }
                     }
                  }
                  if (locatedPage > 0) {
                     jumpToPage(locatedPage);
                     scrollFirstHitIntoView(locatedPage);
                     return;
                  }
                  applySpanHighlights(page, normalized, fallbackTokensRef.current, 8);
               }
            }, 60);
            if (numPages > 1) {
               [page - 1, page + 1]
                  .filter((p) => p >= 1 && p <= numPages)
                  .forEach((nearPage, idx) => {
                     window.setTimeout(async () => {
                        const warmOk = await applyPdfJsHighlights(
                           nearPage,
                           normalized,
                           fallbackTokensRef.current,
                           { silent: true, maxMatches: 2 }
                        );
                        if (!warmOk) {
                           applySpanHighlights(
                              nearPage,
                              normalized,
                              fallbackTokensRef.current,
                              4,
                              { silent: true, maxMatches: 2 }
                           );
                        }
                     }, 140 + idx * 70);
                  });
            }
            window.setTimeout(() => {
               scrollFirstHitIntoView(page);
            }, 140);
         },
         /** BBox-based 精确高亮：跳过文本搜索，直接按坐标渲染矩形 overlay。 */
         highlightBboxes: (page: number, bboxes: BBoxData[]) => {
            if (!bboxes.length || page == null || page < 0) return;

            // 1. 跳到"主"目标页用于滚动定位；【不再清空】其它页已有高亮
            setHighlightStatus('idle');
            markPageRendered(page);
            bboxes.forEach((b) => {
               const p = b.page ?? page;
               if (p) markPageRendered(p);
            });
            jumpToPage(page);

            // 2. 等 DOM 渲染完（pdfjs 异步），再读每页真实宽度
            window.setTimeout(() => {
               if (!containerRef.current) return;

               // 2a. 按 b.page 分组（缺 page 的归到传入的 page 兜底）
               const groups = new Map<number, BBoxData[]>();
               bboxes.forEach((b) => {
                  const p = b.page ?? page;
                  if (!groups.has(p)) groups.set(p, []);
                  groups.get(p)!.push(b);
               });

               // 2b. 逐页：查该页 DOM → 算 scaleFactor → 生成高亮框
               const newBoxesByPage: Record<number, HighlightBox[]> = {};
               groups.forEach((groupBboxes, pageNum) => {
                  // 用 data-page-num 选中该页根元素（不是 canvas，是 pageItem div）
                  const pageEl = containerRef.current?.querySelector(
                     `[data-page-num="${pageNum}"]`
                  ) as HTMLElement | null;
                  if (!pageEl) return;                 // 该页还没渲染，跳过
                  const renderedWidth = pageEl.clientWidth;
                  if (renderedWidth <= 0) return;

                  // 原生 PDF 点宽：后端 b.pageWidth 优先 → pageDimRef 兜底 → 595
                  const nativeWidth =
                     groupBboxes[0].pageWidth ||
                     pageDimRef.current[pageNum]?.width ||
                     595;
                  const scaleFactor = renderedWidth / nativeWidth;

                  newBoxesByPage[pageNum] = groupBboxes.map((b, idx) => ({
                     left: b.x0 * scaleFactor,
                     top: b.top * scaleFactor,
                     width: Math.max(1, (b.x1 - b.x0) * scaleFactor),
                     height: Math.max(1, (b.bottom - b.top) * scaleFactor),
                     // 仅"主目标页"的第一个框是 primary（更醒目 + 滚动锚点）
                     primary: pageNum === page && idx === 0,
                  }));
               });

               // 2c. 合并进现有高亮（保留其它页），而非清空全部
               setHighlightBoxesByPage((prev) => ({ ...prev, ...newBoxesByPage }));
               setHighlightStatus('exact_hit');
               const total = Object.values(newBoxesByPage).reduce((s, a) => s + a.length, 0);
               console.info('[pdf-highlight] engine=bbox groups=%s boxes=%s', groups.size, total);

               // 3. 滚动到主目标页第一个高亮
               window.setTimeout(() => {
                  const hit = containerRef.current?.querySelector(
                     `[data-page-num="${page}"] [data-overlay-hit="1"]`
                  ) as HTMLElement | null;
                  if (hit) {
                     hit.scrollIntoView({ behavior: 'smooth', block: 'center', inline: 'nearest' });
                  }
               }, 60);
            }, 100);
         },
      }),
      [jumpToPage, applySecondaryPageMatch, scrollFirstHitIntoView, applySpanHighlights, applyPdfJsHighlights, numPages, containerRef, markPageRendered]
   );

   return (
      <div className={styles.pdfPanel}>
         {isPdf ? (
            <>
               <div className={styles.pdfScrollArea} ref={containerRef}>
                  <Document
                     key={fileUrl}
                     file={documentFile}
                     onLoadSuccess={(pdf) => {
                        pdfDocRef.current = pdf;
                        const fillDims = async () => {                          // ① 定义一个异步函数
                           const dims: Record<number, { width: number; height: number }> = {};
                           for (let n = 1; n <= pdf.numPages; n++) {
                              const vp = (await pdf.getPage(n)).getViewport({ scale: 1 }); // ② 拿第n页"原生点尺寸"
                              dims[n] = { width: vp.width, height: vp.height };
                           }
                           pageDimRef.current = dims;                            // ③ 一次性写进 ref
                        };
                        fillDims();
                        pageTextIndexCacheRef.current = {};
                        setNumPages(pdf.numPages);
                     }}
                     onLoadError={(err) => {
                        pdfDocRef.current = null;
                        console.error('PDF 加载错误:', err);
                        setPreviewFailed(true);
                     }}
                     loading={
                        <Spin size='large' style={{ marginTop: '20%' }} />
                     }
                     error={<Empty description='加载失败' />}
                  >
                     {renderPages()}
                  </Document>
               </div>

               <PdfToolbar
                  scale={scale}
                  currentPage={currentPage}
                  numPages={numPages}
                  onZoomIn={zoomIn}
                  onZoomOut={zoomOut}
                  onResetZoom={resetZoom}
                  onJumpToPage={jumpToPage}
               />
            </>
         ) : (
            <div
               style={{
                  display: 'flex',
                  flexDirection: 'column',
                  alignItems: 'center',
                  justifyContent: 'center',
                  height: '100%',
                  padding: 24,
                  gap: 12,
               }}
            >
               <Empty description='当前文件暂不支持在线预览' />
               <a
                  href={fileUrl}
                  target='_blank'
                  rel='noreferrer'
                  style={{ fontSize: 14 }}
               >
                  点击下载文件，在本地使用 Word 等工具查看
               </a>
            </div>
         )}
      </div>
   );
});

export default React.memo(PdfPreview);
