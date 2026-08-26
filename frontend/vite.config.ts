/// <reference types="vitest/config" />
import { defineConfig } from 'vite';
import react from '@vitejs/plugin-react';
import path from 'path';

const javaApiTarget = process.env.AIBID_JAVA_BASE_URL || 'http://127.0.0.1:3000';
const frontendPort = Number(process.env.AIBID_FRONTEND_PORT || 5173);
// 子路径部署基路径：默认 '/'，生产构建通过 AIBID_BASE_PATH=/aibid/ 注入
const basePath = process.env.AIBID_BASE_PATH || '/';

// https://vite.dev/config/
export default defineConfig({
   base: basePath,
   plugins: [react()],
   resolve: {
      alias: {
         '@': path.resolve(__dirname, './src'),
      },
   },
   server: {
      host: '127.0.0.1',
      port: frontendPort,
      strictPort: false,
      proxy: {
         // SSE 端点 — 优先匹配，禁用缓冲确保事件实时推送
         '/api/chat/stream': {
            target: javaApiTarget,
            changeOrigin: true,
            selfHandleResponse: true,
            configure: (proxy) => {
               proxy.on('proxyRes', (proxyRes, _req, res) => {
                  // Handle upstream errors
                  proxyRes.on('error', () => {
                     if (!res.headersSent) {
                        res.writeHead(502);
                        res.end(JSON.stringify({ message: 'Upstream error' }));
                     }
                  });
                  // Handle client disconnect
                  res.on('close', () => {
                     proxyRes.destroy();
                  });
                  res.writeHead(proxyRes.statusCode || 200, {
                     'Content-Type': 'text/event-stream',
                     'Cache-Control': 'no-cache',
                     'Connection': 'keep-alive',
                     ...proxyRes.headers,
                  });
                  proxyRes.pipe(res);
               });
               // Handle proxy-level errors (e.g. connection refused)
               proxy.on('error', (_err, _req, res: any) => {
                  if (res?.writeHead) {
                     res.writeHead(502, { 'Content-Type': 'application/json' });
                     res.end(JSON.stringify({ message: 'Proxy error' }));
                  }
               });
            },
         },
         '/api': {
            target: javaApiTarget,
            changeOrigin: true,
         },
      },
   },
   test: {
      globals: true,
      environment: 'jsdom',
      setupFiles: './src/test/setup.ts',
      css: true,
   },
});
