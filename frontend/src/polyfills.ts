/**
 * URL.parse 是较新的静态方法（Chrome 126+ / Firefox 126+ / Safari 17.2+ 才支持）。
 *
 * react-pdf 10.4.1 锁定的 pdfjs-dist 5.4.296 在渲染 PDF 时会【无条件】调用
 * URL.parse（`isValidFetchUrl` / `getResponseOrigin` / 字体与字体映射 URL 解析等处），
 * 老移动端内核 / 内嵌 WebView（如旧安卓 WebView、旧 iOS Safari、微信内置浏览器）
 * 没有这个 API，会直接抛：
 *
 *   TypeError: URL.parse is not a function
 *
 * 进而让整个「标书审核详情页」白屏（其它页正常，因为只有详情页懒加载了 pdf.js）。
 *
 * 处理：在入口最先执行一个语义完全一致的垫片 —— 解析成功返回 URL，非法则返回 null，
 * 与原生 URL.parse 行为相同。不可用 `new URL()` 直接替换调用点，因为源码在 node_modules
 * 的压缩产物里，不便改动；垫片是零侵入的最小方案。
 */
/* eslint-disable @typescript-eslint/no-explicit-any */
// 1) URL.parse（Chrome 126+）—— pdf.js 解析资源 URL 时无条件调用，缺失即白屏
if (typeof (URL as any).parse !== 'function') {
   (URL as any).parse = function (input: string, base?: string | URL): URL | null {
      try {
         return base !== undefined ? new URL(input, base as string) : new URL(input);
      } catch {
         return null;
      }
   };
}

// 2) Promise.withResolvers（Chrome 119+）—— pdf.js 大量使用（31 处），老内核同样缺失
if (typeof (Promise as any).withResolvers !== 'function') {
   (Promise as any).withResolvers = function () {
      let resolve!: (value: unknown) => void;
      let reject!: (reason?: unknown) => void;
      const promise = new Promise<unknown>((res, rej) => {
         resolve = res;
         reject = rej;
      });
      return { promise, resolve, reject };
   };
}

// 3) Array.prototype.at（Chrome 92+）—— pdf.js 用 `.at(-1)` 取尾元素，兜底以防更老的 WebView
if (typeof Array.prototype.at !== 'function') {
   Array.prototype.at = function (index: number) {
      const len = this.length;
      const k = (Math.trunc(index) || 0) < 0 ? len + (Math.trunc(index) || 0) : (Math.trunc(index) || 0);
      return k < 0 || k >= len ? undefined : this[k];
   };
}