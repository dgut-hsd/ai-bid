import React, { useMemo, useRef, useEffect } from 'react';
import { Segmented, Typography, Tag, Space, Alert } from 'antd';
import { useStyles } from '../../style';
import type { AuditIssue } from '../../types';
import type { BBoxData } from '../../components/PDFPreview/PdfPreview';
import { mapBBoxEntries } from './bboxMapping';
import { agentLabel, SEVERITY_MAP } from '@/types/audit';
import { useUrlState } from '@/hooks/useUrlState';

const { Text, Paragraph } = Typography;

type ParsedIssueText = {
   title?: string;
   rationale?: string;
   suggestions: string[];
};

const normalizeAiText = (value: string): string =>
   value
      .replace(/[“”]/g, '"')
      .replace(/[‘’]/g, "'")
      .replace(/\u00A0/g, ' ')
      .trim();

const escapeRegExp = (value: string): string =>
   String(value || '').replace(/[.*+?^${}()|[\]\\]/g, '\\$&');

const cleanJsonLikeNoise = (value: string): string =>
   normalizeAiText(value)
      .replace(/^\s*\[\s*\{?/, '')
      .replace(/\}?\s*\]\s*$/, '')
      .replace(/"\s*title"\s*:\s*/gi, '')
      .replace(/"\s*severity"\s*:\s*"[^"]*"\s*,?/gi, '')
      .replace(/"\s*rationale"\s*:\s*/gi, '')
      .replace(/"\s*suggestions"\s*:\s*\[[\s\S]*$/gi, '')
      .replace(/[{},[\]]+/g, ' ')
      .replace(/\s{2,}/g, ' ')
      .trim();

const parseIssueText = (raw: string): ParsedIssueText | null => {
   const text = normalizeAiText(raw || '');
   if (!text) return null;

   const tryParse = (value: string): unknown => {
      try {
         return JSON.parse(value);
      } catch {
         return null;
      }
   };

   let parsed = tryParse(text);
   if (!parsed) {
      const start = text.indexOf('[');
      const end = text.lastIndexOf(']');
      if (start >= 0 && end > start) {
         parsed = tryParse(text.slice(start, end + 1));
      }
   }

   const first =
      Array.isArray(parsed) && parsed.length > 0
         ? (parsed[0] as Record<string, unknown>)
         : null;
   if (!first) {
      const markerTitleMatch = text.match(
         /[【\[]\s*问题标题\s*[】\]]\s*[:：]?\s*([\s\S]*?)(?=(?:\n\s*[【\[]\s*问题说明\s*[】\]])|$)/i
      );
      const markerRationaleMatches = Array.from(
         text.matchAll(
            /[【\[]\s*问题说明\s*[】\]]\s*[:：]?\s*([\s\S]*?)(?=(?:\n\s*[【\[]\s*[^\]]+[】\]])|$)/gi
         )
      );
      const titleMatch = text.match(/"title"\s*:\s*"([^"]+)"/i);
      const rationaleMatch = text.match(
         /"rationale"\s*:\s*"([\s\S]*?)"\s*,\s*"suggestions"/i
      );
      const suggestionsBlock = text.match(/"suggestions"\s*:\s*\[([\s\S]*?)\]/i);
      const suggestions = suggestionsBlock
         ? Array.from(suggestionsBlock[1].matchAll(/"([^"]+)"/g)).map((m) => m[1])
         : [];
      const markerTitle = markerTitleMatch?.[1]?.trim();
      const markerRationale = markerRationaleMatches
         .map((match) => String(match[1] || '').trim())
         .filter(Boolean)
         .pop();
      if (
         !titleMatch &&
         !rationaleMatch &&
         suggestions.length === 0 &&
         !markerTitle &&
         !markerRationale
      ) {
         return null;
      }
      return {
         title: titleMatch?.[1] || markerTitle,
         rationale:
            rationaleMatch?.[1] || markerRationale || cleanJsonLikeNoise(text),
         suggestions,
      };
   }

   const suggestions = Array.isArray(first.suggestions)
      ? first.suggestions
           .map((item) => String(item ?? '').trim())
           .filter(Boolean)
      : [];

   return {
      title: typeof first.title === 'string' ? first.title : undefined,
      rationale: typeof first.rationale === 'string' ? first.rationale : undefined,
      suggestions,
   };
};

const normalizeIssueTitle = (value?: string): string => {
   const text = String(value || '').trim();
   if (!text) return '';
   if (text === '[]' || text === '[ ]' || text === '【】') return '';
   const cleaned = text
      .replace(/^\s+|\s+$/g, '')
      .replaceAll('[', '')
      .replaceAll(']', '')
      .replaceAll('【', '')
      .replaceAll('】', '')
      .trim();
   if (!cleaned) return '';
   if (/^(null|undefined|none)$/i.test(cleaned)) return '';
   return cleaned;
};

const hasMeaningfulContent = (title: string, rationale?: string): boolean => {
   const normalizedRationale = normalizeIssueTitle(rationale || '');
   if (title) return true;
   if (normalizedRationale && normalizedRationale.length >= 6) return true;
   return false;
};

const isGenericRationale = (value?: string): boolean => {
   const text = normalizeIssueTitle(value || '');
   if (!text) return true;
   if (text.length < 22) return true;
   const genericPatterns = [
      /未在当前文档证据中发现/i,
      /存在遗漏风险/i,
      /可预期性不足/i,
      /不透明/i,
      /需进一步核验/i,
      /建议补充/i,
   ];
   return genericPatterns.some((pattern) => pattern.test(text));
};

const normalizeRationaleForDisplay = (value?: string): string => {
   const text = sanitizeDisplayText(value);
   if (!text) return '';
   let normalized = text;
   const rationaleMarker = '【问题说明】';
   const lastRationaleMarker = normalized.lastIndexOf(rationaleMarker);
   if (lastRationaleMarker >= 0) {
      const tail = normalized
         .slice(lastRationaleMarker + rationaleMarker.length)
         .trim();
      if (tail) {
         normalized = tail;
      }
   }
   normalized = normalized
      .replace(/[【\[]\s*问题标题\s*[】\]]\s*[:：]?[^\n\r]*(?:[\r\n]+|$)/gi, '')
      .replace(/[【\[]\s*问题定位\s*[】\]]\s*第?\s*\d+\s*页?\s*[:：]/gi, '')
      .replace(/[【\[]\s*问题定位\s*[】\]]\s*[:：][\s\S]*?(?=(?:[【\[]\s*问题说明\s*[】\]])|$)/gi, '')
      .replace(/主证据中，?\s*[【\[]\s*问题定位\s*[】\]]/gi, '')
      .replace(/[【\[]\s*问题定位\s*[】\]]/gi, '')
      .trim();
   if (/^【问题说明】/.test(normalized)) {
      normalized = normalized.replace(/^【问题说明】\s*/, '').trim();
   }
   normalized = normalized.replace(/^\s*第?\s*\d+\s*页?\s*[:：]\s*/i, '').trim();
   if (!/[。！？!?]$/.test(normalized)) {
      normalized = `${normalized}。`;
   }
   return normalized;
};

const shouldRenderIssue = (issue: AuditIssue): boolean => {
   const parsed = parseIssueText(issue.description || '');
   const rawDescription = sanitizeDisplayText(issue.description);
   const title = normalizeIssueTitle(parsed?.title || '审查问题');
   const normalizedRationale = normalizeRationaleForDisplay(
      parsed?.rationale || rawDescription
   );
   if (isGenericRationale(normalizedRationale)) {
      const hasLocalAnchor =
         Boolean(parsePageNumber(issue.location?.pageNumber)) ||
         String(issue.location?.context || '').trim().length > 0;
      if (!hasLocalAnchor) {
         return false;
      }
   }
   return hasMeaningfulContent(title, normalizedRationale);
};

const sanitizeDisplayText = (value?: string): string => {
   const text = String(value || '').trim();
   if (!text) return '';
   return text
      .replace(/主证据/g, '审核文件')
      .replace(/\bDocuments?\b/gi, '标书内容')
      .replace(/\bStandards?\b/gi, '政策依据')
      .replace(/\bTopic\b/gi, '审查主题')
      .trim();
};

const parsePageNumber = (value: unknown): number | null => {
   if (typeof value === 'number' && Number.isFinite(value) && value >= 0) {
      return Math.floor(value);
   }
   if (typeof value === 'string') {
      const num = Number.parseInt(value, 10);
      if (Number.isFinite(num) && num >= 0) {
         return num;
      }
   }
   return null;
};

const normalizeLocatePage = (page: number, sourceFileName?: string): number => {
   void sourceFileName;
   return page;
};

type ParsedSourceRef = {
   fileName: string;
   fileId?: number;
   sourceType?: 'knowledge' | 'tender' | 'unknown';
   previewUrl?: string;
};

const parseSourceReference = (reference?: string): ParsedSourceRef | null => {
   const value = (reference || '').trim();
   if (!value.toLowerCase().startsWith('source://')) {
      return null;
   }
   const match = value.match(/^source:\/\/([^/]+)\/([^/]+)\/(.+)$/i);
   if (!match) {
      return null;
   }
   const sourceTypeRaw = match[1].toLowerCase();
   const sourceType =
      sourceTypeRaw === 'knowledge' || sourceTypeRaw === 'tender'
         ? sourceTypeRaw
         : 'unknown';
   const fileIdNum = Number.parseInt(match[2], 10);
   const fileId = Number.isFinite(fileIdNum) && fileIdNum > 0 ? fileIdNum : undefined;
   let fileName = match[3];
   try {
      fileName = decodeURIComponent(fileName);
   } catch {
      fileName = match[3];
   }
   const normalized = String(fileName || '').replace(/\\/g, '/');
   const nameParts = normalized.split('/').filter(Boolean);
   const baseName = nameParts.length ? nameParts[nameParts.length - 1] : String(fileName || '');
   const previewUrl =
      sourceType === 'knowledge' && fileId
         ? `${import.meta.env.VITE_API_BASE_URL}/api/knowledge-files/${fileId}/preview`
         : fileId
            ? `${import.meta.env.VITE_API_BASE_URL}/api/bid-documents/${fileId}/download`
            : undefined;
   return {
      fileName: baseName || '未返回来源文件',
      fileId,
      sourceType,
      previewUrl,
   };
};

const extractSourceInfo = (
   issue: AuditIssue,
   currentFileName?: string,
   currentFileId?: number
): ParsedSourceRef => {
   void currentFileName;
   void currentFileId;
   const parsedRef = parseSourceReference(issue.reference);
   if (parsedRef && parsedRef.sourceType === 'knowledge') {
      return {
         fileName: '知识库文档',
         fileId: parsedRef.fileId,
         sourceType: 'knowledge',
         previewUrl: parsedRef.previewUrl,
      };
   }
   return {
      fileName: '知识库文档',
      sourceType: 'knowledge',
   };
};

const buildHighlightText = (
   issue: AuditIssue,
   rationale: string,
   title: string
): string => {
   const anchorChars = Array.isArray(issue.anchorCharsRange) ? issue.anchorCharsRange : [];
   const anchorQuote = String(issue.anchorQuote || '').trim();
   if (anchorQuote.length >= 12 && anchorChars.length >= 2) {
      const start = Math.max(0, Number(anchorChars[0]) || 0);
      const end = Math.max(start + 1, Number(anchorChars[1]) || start + 1);
      const mid = Math.floor((start + end) / 2);
      const left = Math.max(0, mid - 20);
      const right = Math.min(anchorQuote.length, left + 40);
      const focused = anchorQuote.slice(left, right).trim();
      if (focused.length >= 8) {
         return focused;
      }
   }
   if (anchorQuote.length >= 6) {
      return anchorQuote.slice(0, 60);
   }
   const context = String(issue.location?.context || '').trim();
   if (context.length >= 6) {
      return context.slice(0, 120);
   }
   const rationaleSentence = String(rationale || '')
      .split(/[。！？!?；;]/)
      .map((s) => s.trim())
      .find((s) => s.length >= 6);
   if (rationaleSentence) {
      return rationaleSentence.slice(0, 120);
   }
   return String(title || '').trim().slice(0, 80);
};

const buildAnchorPrefix = (issue: AuditIssue): string => {
   const quote = String(issue.anchorQuote || issue.location?.context || '').trim();
   if (!quote) {
      return '【问题定位】原文片段待定位';
   }
   const shortQuote = quote.length > 160 ? `${quote.slice(0, 160)}...` : quote;
   return `【问题定位】"${shortQuote}"`;
};

const compactForCompare = (value: string): string =>
   String(value || '')
      .replace(/[\s，,。；;：:!?！？、（）()\[\]【】"“”'‘’]/g, '')
      .trim()
      .toLowerCase();

const removeAnchorOverlapPrefix = (text: string, anchor: string): string => {
   const rawText = String(text || '').trim();
   const rawAnchor = String(anchor || '').trim();
   if (!rawText || !rawAnchor) return rawText;

   const compactText = compactForCompare(rawText);
   const compactAnchor = compactForCompare(rawAnchor);
   if (compactText.length < 8 || compactAnchor.length < 8) return rawText;

   const minOverlap = 6; // Reduced to catch smaller overlaps like "的原则进"
   const maxProbe = Math.min(compactText.length, 160);
   let overlapCompact = '';

   // 1) 优先匹配：文本前缀等于锚点后缀（连续复述场景）
   for (let i = 0; i < compactAnchor.length; i++) {
      const suffix = compactAnchor.slice(i);
      if (suffix.length < minOverlap) continue;
      if (compactText.startsWith(suffix) && suffix.length > overlapCompact.length) {
         overlapCompact = suffix;
      }
   }

   // 2) 兜底匹配：文本前缀在锚点任意位置出现（断句/截断续写场景）
   if (!overlapCompact) {
      for (let len = maxProbe; len >= minOverlap; len--) {
         const prefix = compactText.slice(0, len);
         if (compactAnchor.includes(prefix)) {
            overlapCompact = prefix;
            break;
         }
      }
   }
   if (!overlapCompact) return rawText;

   const ignoreCharPattern = /[\s，,。；;：:!?！？、（）()\[\]【】"“”'‘’]/;
   let pointer = 0;
   let endIndex = -1;
   for (let i = 0; i < rawText.length; i++) {
      const ch = rawText[i];
      if (ignoreCharPattern.test(ch)) {
         continue;
      }
      if (pointer >= overlapCompact.length) {
         endIndex = i;
         break;
      }
      if (ch.toLowerCase() !== overlapCompact[pointer]) {
         return rawText;
      }
      pointer += 1;
      if (pointer === overlapCompact.length) {
         endIndex = i + 1;
         break;
      }
   }
   if (pointer < overlapCompact.length || endIndex < 0) {
      return rawText;
   }
   const stripped = rawText
      .slice(endIndex)
      .replace(/^[\s，,。；;：:!?！？、”"’']+/, '')
      .trim();
   return stripped || rawText;
};

// const trimToAnalyticalStart = (value: string): string => {
//    const text = String(value || '').trim();
//    if (!text) return '';
//    const sentences = text.match(/[^。！？!?；;\n]+[。！？!?；;\n]*/g) || [text];
//    if (sentences.length < 2) return text;

//    const analysisPattern =
//       /(风险|冲突|不一致|不完整|缺失|不明确|可执行|可预期|合规|责任边界|责任分配|预算备案|建议补充|建议明确|建议细化|建议约定|建议统一|应当|需要|需补充|需明确|需细化|可能导致|易引发|不利影响)/;

//    const sentenceLooksAnalytical = (sentence: string): boolean => {
//       const normalized = sentence
//          .replace(/^[“"'【\[]+/, '')
//          .replace(/[”"'】\]]+$/, '')
//          .trim();
//       if (normalized.length < 8) return false;
//       const quoteLikePattern =
//          /(须知前附表|序号\s*条款名称|内容及要求|本表与招标文件|以本表为准|第[一二三四五六七八九十\d]+[章节条款]|采购需求|开标一览表|分项报价表)/;
//       if (quoteLikePattern.test(normalized)) {
//          return false;
//       }
//       return analysisPattern.test(normalized);
//    };

//    let startIndex = -1;
//    for (let i = 0; i < sentences.length; i++) {
//       if (sentenceLooksAnalytical(sentences[i])) {
//          startIndex = i;
//          break;
//       }
//    }
//    if (startIndex <= 0) return text;

//    const prefix = sentences.slice(0, startIndex).join('').trim();
//    if (compactForCompare(prefix).length < 18) return text;
//    const trimmed = sentences.slice(startIndex).join('').trim();
//    return trimmed || text;
// };

// const cutToExplicitAnalyticalClause = (value: string): string => {
//    const text = String(value || '').trim();
//    if (!text) return '';
//    const anchors = [
//       '审核文件仅',
//       '审核文件未',
//       '该条款',
//       '存在',
//       '违反',
//       '建议',
//       '应当',
//       '需要',
//       '需补充',
//       '需明确',
//       '需细化',
//       '可能导致',
//       '易引发',
//    ];
//    let hit = -1;
//    for (const anchor of anchors) {
//       const idx = text.indexOf(anchor);
//       if (idx > 0 && (hit < 0 || idx < hit)) {
//          hit = idx;
//       }
//    }
//    if (hit <= 0) return text;
//    const prefix = text.slice(0, hit).trim();
//    if (compactForCompare(prefix).length < 20) return text;
//    const trimmed = text.slice(hit).replace(/^[，,。；;：:\s]+/, '').trim();
//    return trimmed || text;
// };

const buildAnchorKey = (issue: AuditIssue): string => {
   const quote = String(issue.anchorQuote || issue.location?.context || '').trim();
   const compact = compactForCompare(quote);
   if (compact.length >= 10) return compact.slice(0, 80);
   const fallback = compactForCompare(String(issue.description || '').slice(0, 100));
   return fallback.slice(0, 80);
};

const buildIssueExplanation = (issue: AuditIssue, raw?: string): string => {
   let text = normalizeRationaleForDisplay(raw)
      .replace(/【问题说明】/g, '')
      .replace(/^主证据中，?\s*/g, '')
      .replace(/^审核文件中，?\s*/g, '')
      .replace(/📎\s*搜索来源[\s\S]*$/g, '')  // 去掉末尾搜索来源（与 CitationList 重复）
      .trim();

   const anchor = String(issue.anchorQuote || '').trim();
   if (anchor && text) {
      const anchorShort = anchor.slice(0, Math.min(anchor.length, 28));
      const cAnchor = compactForCompare(anchorShort);
      const cText = compactForCompare(text);
      if (cAnchor && cText.startsWith(cAnchor)) {
         text = text
            .replace(/^[“"][^”"]{4,260}[”"]\s*[，,。；;:：]?\s*/, '')
            .trim();
         if (compactForCompare(text).startsWith(cAnchor)) {
            text = text
               .replace(
                  new RegExp(
                     `^${escapeRegExp(anchorShort)}[\\s，,。；;:：-]*`,
                     'i'
                  ),
                  ''
               )
               .trim();
         }
      }
   }
   if (anchor && text) {
      text = removeAnchorOverlapPrefix(text, anchor);
   }

   if (!text) {
      text = '该条款与合同执行或合规要求不一致，需要按证据片段补充可执行约束与责任条款。';
   }
   if (!/[。！？!?]$/.test(text)) {
      text = `${text}。`;
   }
   return text;
};

const EVIDENCE_VERDICT_META: Record<string, { label: string; color: string }> = {
   refute: { label: '被反驳', color: 'error' },
   insufficient: { label: '证据不足', color: 'default' },
};

interface AnalysisListProps {
   issues: AuditIssue[];
   isComplete: boolean;
   onLocateIssuePage: (page: number, highlightText?: string, fallbackTokens?: string[]) => void;
   currentFileName?: string;
   currentFileId?: number;
   /** 点击风险卡打开推理抽屉 */
   onIssueClick?: (issue: AuditIssue) => void;
   /** 审核任务 ID（用于 bbox API 调用） */
   taskId?: string | null;
   /** BBox-based 精确高亮回调（优先于文本匹配） */
   onLocateBboxes?: (page: number, bboxes: BBoxData[], highlightText?: string, fallbackTokens?: string[]) => void;
}

/** 高亮模式配置：auto=优先BBox失败回落 | bbox=仅BBox | text=仅文本匹配 */
const HIGHLIGHT_MODE: string = import.meta.env.VITE_HIGHLIGHT_MODE || 'auto';

/** 调用 Java 代理端点获取 block BBox 坐标 */
async function fetchBlockBboxes(taskId: string, blockIds: string[]): Promise<BBoxData[]> {
  const baseUrl = import.meta.env.VITE_API_BASE_URL || '';
  const ids = blockIds.slice(0, 10).join(',');
  const url = `${baseUrl}/api/audit-tasks/${taskId}/blocks?ids=${encodeURIComponent(ids)}`;
  console.info('[bbox-fetch] calling: %s', url);
  const token = localStorage.getItem('token') || sessionStorage.getItem('token') || '';
  const resp = await fetch(url, {
    headers: { Authorization: token ? `Bearer ${token}` : '' },
  });
  if (!resp.ok) throw new Error(`HTTP ${resp.status}`);
  const json = await resp.json();
  const list = json?.data || [];
  console.info('[bbox-fetch] got %d bbox entries for %d blockIds', list.length, blockIds.length);
  return mapBBoxEntries(list);
}

export const AnalysisList: React.FC<AnalysisListProps> = React.memo(
   ({ issues, isComplete, onLocateIssuePage, currentFileName, currentFileId, onIssueClick, taskId, onLocateBboxes }) => {
      const { theme, styles } = useStyles();
      // 初始 tab：若进入时审核已完成，默认直接落在「高风险」；审核进行中进入则默认「全部」。
      const [queryParams, setQueryParams] = useUrlState({ tab: isComplete ? 'high' : 'all' });
      const currentTab = queryParams.tab;

      // 「未完成 → 完成」瞬间自动切「高风险」（适用于审核中已停留在结果页的场景），
      // 只在转换时刻切一次，之后尊重用户手动切换。
      const prevCompleteRef = useRef(isComplete);
      useEffect(() => {
         if (isComplete && !prevCompleteRef.current) {
            setQueryParams({ tab: 'high' });
         }
         prevCompleteRef.current = isComplete;
      }, [isComplete]);

      const visibleIssues = useMemo(
         () => (issues || []).filter((i) => i && shouldRenderIssue(i)),
         [issues]
      );
      const visibleSummary = useMemo(
         () => ({
            high: visibleIssues.filter((i) => i?.severity === 'high').length,
            medium: visibleIssues.filter((i) => i?.severity === 'medium').length,
            low: visibleIssues.filter((i) => i?.severity === 'low').length,
            info: visibleIssues.filter((i) => i?.severity === 'info').length,
         }),
         [visibleIssues]
      );

      const filteredIssues = useMemo(() => {
         if (currentTab === 'high')
            return visibleIssues.filter((i) => i?.severity === 'high');
         if (currentTab === 'medium')
            return visibleIssues.filter((i) => i?.severity === 'medium');
         if (currentTab === 'low')
            return visibleIssues.filter((i) => i?.severity === 'low');
         if (currentTab === 'info')
            return visibleIssues.filter((i) => i?.severity === 'info');
         return visibleIssues;
      }, [visibleIssues, currentTab]);

      // 已播放入场动画的卡 key（跨渲染持久，避免切 tab 重播）
      const animatedKeysRef = useRef<Set<string>>(new Set());
      useEffect(() => {
         filteredIssues.forEach((i, idx) => {
            animatedKeysRef.current.add(i.riskId || i.issueNo || `issue-${idx}`);
         });
      }, [filteredIssues]);

      const canonicalPageByAnchor = useMemo(() => {
         const pageVotes = new Map<string, Map<number, number>>();
         visibleIssues.forEach((issue) => {
            const page =
               parsePageNumber(issue.anchorPage) ??
               parsePageNumber(issue.location?.pageNumber);
            if (page == null) return;
            const key = buildAnchorKey(issue);
            if (!key) return;
            const votes = pageVotes.get(key) || new Map<number, number>();
            votes.set(page, (votes.get(page) || 0) + 1);
            pageVotes.set(key, votes);
         });
         const result = new Map<string, number>();
         pageVotes.forEach((votes, key) => {
            let bestPage = 0;
            let bestCount = -1;
            votes.forEach((count, page) => {
               if (count > bestCount || (count === bestCount && page > 0 && page < bestPage)) {
                  bestCount = count;
                  bestPage = page;
               }
            });
            if (bestPage > 0) result.set(key, bestPage);
         });
         return result;
      }, [visibleIssues]);

      const segOptions = useMemo(
         () => [
            { label: `全部 ${visibleIssues.length}`, value: 'all' },
            { label: `高 ${visibleSummary.high}`, value: 'high' },
            { label: `中 ${visibleSummary.medium}`, value: 'medium' },
            { label: `低 ${visibleSummary.low}`, value: 'low' },
            { label: `信息 ${visibleSummary.info}`, value: 'info' },
         ],
         [visibleIssues.length, visibleSummary]
      );

      const renderedIssueCards = useMemo(() => {
         let newSeq = 0;
         // 统一的"定位"处理器：主卡与成员卡共用，优先 BBox、回落文本匹配
         const createLocateHandler = (target: AuditIssue) => () => {
            const page =
               canonicalPageByAnchor.get(buildAnchorKey(target)) ??
               parsePageNumber(target.anchorPage) ??
               parsePageNumber(target.location?.pageNumber);
            if (page == null) return;
            const src = extractSourceInfo(target, currentFileName, currentFileId);
            const normalizedPage = normalizeLocatePage(page, src.fileName);
            // ★ 首选：后端已算好词级紧致框（highlight_rects），直接按坐标渲染，
            //   无需再 fetch /blocks，也无需走 pdf.js 文本层收敛（避免其固有误差）。
            const preciseRects = Array.isArray(target.highlightRects) ? target.highlightRects : [];
            if (HIGHLIGHT_MODE !== 'text' && preciseRects.length > 0 && onLocateBboxes) {
               const boxes: BBoxData[] = preciseRects.map((r) => ({
                  x0: r.x0,
                  top: r.top,
                  x1: r.x1,
                  bottom: r.bottom,
                  pageWidth: r.pageWidth,
                  page: (r.page ?? 0) + 1, // 后端 page 为 0-based → 前端 1-based data-page-num
               }));
               onLocateBboxes(normalizedPage, boxes);
               return;
            }
            const useBbox =
               HIGHLIGHT_MODE !== 'text' &&
               target.blockIds &&
               target.blockIds.length > 0 &&
               taskId &&
               onLocateBboxes;
            // 文本定位素材提前计算：bbox 路径也要用作「文本层精确收敛」的 source_quote
            const parsedDesc = parseIssueText(target.description);
            const hl = buildHighlightText(
               target,
               buildIssueExplanation(target, parsedDesc?.rationale || sanitizeDisplayText(target.description)),
               target.category || '审查问题'
            );
            const tokens = Array.isArray(target.anchorTokens)
               ? target.anchorTokens.map((t) => String(t || '').trim()).filter(Boolean).slice(0, 5)
               : [];
            const fallback = () => {
               onLocateIssuePage(normalizedPage, hl, tokens);
            };
            if (useBbox) {
               fetchBlockBboxes(taskId!, target.blockIds!)
                  .then((bboxes) => {
                     if (bboxes.length > 0) onLocateBboxes!(normalizedPage, bboxes, hl, tokens);
                     else if (HIGHLIGHT_MODE === 'auto') fallback();
                  })
                  .catch(() => {
                     if (HIGHLIGHT_MODE === 'auto') fallback();
                  });
               return;
            }
            fallback();
         };
         return filteredIssues
            .map((issue, issueIndex) => {
               const parsed = parseIssueText(issue.description);
               const cardKey = issue.riskId || issue.issueNo || `issue-${issueIndex}`;
               const isNew = !animatedKeysRef.current.has(cardKey);
               const stepDelay = isNew ? Math.min(newSeq++, 20) * 200 : 0;
               const rawDescription = sanitizeDisplayText(issue.description);
               const title = issue.category || '审查问题';
               const rationaleBody = buildIssueExplanation(
                  issue,
                  parsed?.rationale || rawDescription
               );
               const verdictMeta = issue.evidenceVerdict ? EVIDENCE_VERDICT_META[issue.evidenceVerdict] : undefined;
               const isVerifierSupported = issue.evidenceVerdict === 'support' && Boolean((issue.verifierReason || '').trim());
               const rationale = isVerifierSupported ? `${buildAnchorPrefix(issue)}\n【证据核验】${String(issue.verifierReason || '').trim()}` : `${buildAnchorPrefix(issue)}\n【问题说明】${rationaleBody}`;
               if (!hasMeaningfulContent(title, rationale)) {
                  return null;
               }
               const sourceInfo = extractSourceInfo(issue, currentFileName, currentFileId);

               const issueAnchorKey = buildAnchorKey(issue);
               const canonicalPage =
                  (issueAnchorKey ? canonicalPageByAnchor.get(issueAnchorKey) : undefined) || null;
               const rawPageNo =
                  canonicalPage ??
                  parsePageNumber(issue.anchorPage) ??
                  parsePageNumber(issue.location?.pageNumber);
               const pageNo = rawPageNo != null
                  ? normalizeLocatePage(rawPageNo, sourceInfo.fileName)
                  : null;
               const issueRenderKey = cardKey;

               const handleLocate = createLocateHandler(issue);

               return (
                  <div
                     key={issueRenderKey}
                     onClick={() => { onIssueClick?.(issue); handleLocate(); }}
                     style={{
                        padding: '14px 16px 10px',
                        border: `1px solid ${theme.colorBorderSecondary}`,
                        borderRadius: 8,
                        cursor: onIssueClick ? 'pointer' : undefined,
                        background: theme.colorBgContainer,
                        transition: 'box-shadow 0.2s',
                        ...(isNew
                           ? { animation: 'issueCardIn 0.42s ease-out both', animationDelay: `${stepDelay}ms` }
                           : {}),
                     }}
                     onMouseEnter={(e) => {
                        e.currentTarget.style.boxShadow = '0 2px 8px rgba(0,0,0,0.06)';
                     }}
                     onMouseLeave={(e) => {
                        e.currentTarget.style.boxShadow = 'none';
                     }}
                  >
                     <div
                        onClick={(e) => {
                           e.stopPropagation();
                           handleLocate();
                        }}
                        style={{
                           display: 'flex',
                           alignItems: 'center',
                           justifyContent: 'space-between',
                           marginBottom: 8,
                           cursor: pageNo ? 'pointer' : 'default',
                        }}
                     >
                        <Space size={8}>
                           <Tag
                              style={{ fontSize: '1rem' }}
                              color="green"
                           >
                              {pageNo ? `第 ${pageNo} 页` : '页码待定位'}
                           </Tag>

                           {title ? (
                              <Text
                                 strong
                                 style={{
                                    fontSize: 15,
                                    letterSpacing: '0.5px',
                                 }}
                              >
                                 {title}
                              </Text>
                           ) : null}
                        </Space>

                        <Space size={4}>
                           {(issue.agentName || issue.agent) && (
                              <Tag style={{ fontSize: 12, margin: 0 }}>
                                 {agentLabel(issue.agentName || issue.agent || '')}
                              </Tag>
                           )}
                           <Tag
                              style={{ fontSize: 12, margin: 0 }}
                              color={
                                 issue.severity === 'high' ? 'error' :
                                 issue.severity === 'medium' ? 'orange' :
                                 issue.severity === 'low' ? 'warning' : 'processing'
                              }
                           >
                              {issue.isCritical ? '重大问题' : SEVERITY_MAP[issue.severity]}
                           </Tag>
                        </Space>
                     </div>

                     {/* 核验结论 + Truncated */}
                     <div style={{ marginTop: 4, marginBottom: 6 }}>
                        {verdictMeta && (
                           <div style={{ display: 'flex', alignItems: 'center', gap: 8 }}>
                              <Tag color={verdictMeta.color} style={{ fontSize: 12, margin: 0 }}>
                                 {verdictMeta.label}
                              </Tag>
                           </div>
                        )}
                        {issue.truncated && (
                           <Alert
                              type="warning"
                              showIcon
                              message="审查未完成 — Agent 轮次耗尽，置信度低，建议人工复核"
                              style={{ marginTop: 4, fontSize: 13 }}
                           />
                        )}
                     </div>

                     {rationale && (
                        <Paragraph
                           style={{
                              marginBottom: 6,
                              color: '#262626',
                              fontWeight: 500,
                              fontSize: 15,
                              lineHeight: 1.85,
                              whiteSpace: 'pre-wrap',
                              fontFamily:
                                 '"Times New Roman","Noto Serif SC","Songti SC",serif',
                           }}
                        >
                           {rationale}
                        </Paragraph>
                     )}
                     {/* P2 回退：同源聚合成员展开块已移除，每条 finding 独立成卡 */}
                  </div>
               );
            })
            .filter(Boolean);
      }, [filteredIssues, theme, onLocateIssuePage, currentFileName, currentFileId, canonicalPageByAnchor, onIssueClick, taskId, onLocateBboxes]);

      return (
         <div
            style={{
               flex: 1,
               display: 'flex',
               flexDirection: 'column',
            }}
         >
            <style>{`
               @keyframes issueCardIn {
                 from { opacity: 0; transform: translateY(18px); }
                 to { opacity: 1; transform: translateY(0); }
               }
            `}</style>
            <div style={{ display: 'flex', alignItems: 'center', gap: 6, padding: '2px 6px 6px', flexShrink: 0 }}>
               <Text type="secondary" style={{ fontSize: 12, whiteSpace: 'nowrap', flexShrink: 0 }}>风险等级</Text>
               <div style={{ flex: 1, minWidth: 0 }} className={styles.severityFilter}>
                  <Segmented
                     block
                     size="small"
                     value={currentTab}
                     onChange={(val) => setQueryParams({ tab: val as string })}
                     options={segOptions}
                  />
               </div>
            </div>

            <div
               style={{
                  flex: 1,
                  minHeight: 0,
                  overflowY: 'auto',
                  scrollbarWidth: 'none',
                  msOverflowStyle: 'none',
               }}
            >
               <div
                  style={{
                     display: 'flex',
                     flexDirection: 'column',
                     gap: 12,
                     paddingRight: 4,
                  }}
               >
                  {renderedIssueCards.length === 0 && isComplete ? (
                     <Text type='secondary' style={{ paddingLeft: 12, fontSize: '1.15rem' }}>
                        暂未发现相关问题
                     </Text>
                  ) : (
                     renderedIssueCards
                  )}
               </div>
            </div>
         </div>
      );
   }
);
