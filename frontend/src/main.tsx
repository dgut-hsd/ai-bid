// 必须在所有业务代码之前运行：老移动端内核缺少 URL.parse，pdf.js 渲染时会调它导致白屏
import './polyfills'
import { StrictMode } from 'react'
import { createRoot } from 'react-dom/client'
import './styles/global.css'
import App from './App.tsx'

createRoot(document.getElementById('root')!).render(
  <StrictMode>
    <App />
  </StrictMode>,
)
