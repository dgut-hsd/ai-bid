# ── 前端多阶段构建：Node 构建 → Nginx 托管 ──
FROM node:22-alpine AS build

# 安装 pnpm，版本与 package.json 的 packageManager 保持一致
RUN npm install -g pnpm@11.20.0

WORKDIR /app

# 先复制依赖清单，利用 Docker 层缓存（依赖没变就不重装）
# pnpm-workspace.yaml 含 allowBuilds（esbuild 放行），必须一起 COPY，否则 pnpm 11 会忽略 esbuild 构建脚本
COPY package.json pnpm-lock.yaml pnpm-workspace.yaml ./

# 按锁文件安装依赖，保证可复现
RUN pnpm install --frozen-lockfile

# 复制源码并构建（tsc 类型检查 + vite 打包）
COPY . .

# 子路径部署基路径（默认 /aibid/，可用 --build-arg BASE_PATH=... 覆盖；根路径部署传 /）
ARG BASE_PATH=/aibid/
ENV AIBID_BASE_PATH=$BASE_PATH

RUN pnpm build

# ── 运行阶段：Nginx 托管静态产物并反代 API ──
FROM nginx:1.27-alpine

# 覆盖默认站点配置（含 /api 反代 + SSE 支持）
COPY nginx.conf /etc/nginx/conf.d/default.conf

# 拷贝构建产物
COPY --from=build /app/dist /usr/share/nginx/html

EXPOSE 80