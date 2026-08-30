# ── Rust 引擎多阶段构建：编译 → 精简运行 ──
# 必须用 bookworm 变体：运行阶段是 debian:bookworm-slim(glibc 2.36)，
# 若用默认 rust:1(基于 Trixie, glibc 2.39)编译，产物会报 GLIBC_2.38/2.39 not found
FROM rust:1-bookworm AS build

# rustup 工具链组件下载走国内镜像，避免 cargo build 触发联网同步时卡在 static.rust-lang.org
ENV RUSTUP_DIST_SERVER=https://rsproxy.cn
ENV RUSTUP_UPDATE_ROOT=https://rsproxy.cn/rustup

# 安装编译期系统依赖：
# - build-essential/cc : cc crate（C 编译器）
# - cmake/ninja/perl/nasm : aws-lc-sys（rustls 加密后端）
# - protobuf-compiler  : qdrant-client 的 tonic/gRPC
# - clang/pkg-config/libssl-dev : 常见 C 库构建
RUN apt-get update && apt-get install -y --no-install-recommends \
    build-essential pkg-config cmake ninja-build perl nasm \
    libssl-dev clang golang-go protobuf-compiler \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /app

# cargo 依赖下载走国内镜像（rsproxy）
RUN mkdir -p /usr/local/cargo && printf '%s\n' \
    '[source.crates-io]' \
    'replace-with = "rsproxy-sparse"' \
    '' \
    '[source.rsproxy-sparse]' \
    'registry = "sparse+https://rsproxy.cn/index/"' \
    '' \
    '[registries.rsproxy]' \
    'index = "sparse+https://rsproxy.cn/index/"' \
    '' \
    '[net]' \
    'git-fetch-with-cli = true' \
    > /usr/local/cargo/config.toml

# 复制依赖清单与源码（src 需同时存在，cargo 才能解析出 target）
COPY Cargo.toml Cargo.lock build.rs rust-toolchain.toml ./
COPY src ./src

# 编译 HTTP 服务入口；registry 与 target 产物跨构建复用（首次全量，之后增量）。
# 注意：target 在 cache mount 中不会进入镜像层，需把产物拷到普通路径供 COPY --from 使用。
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/app/target \
    cargo build --release --bin server \
    && cp /app/target/release/server /app/server-release

# ── 运行阶段 ──
FROM debian:bookworm-slim

# 运行时依赖：ca-certificates（HTTPS）+ libssl3 + libgomp1（ONNX 动态库的 OpenMP）
# LibreOffice 用于 DOCX/DOC → PDF 转换（fonts-noto-cjk 避免中文标书转 PDF 乱码/豆腐块）
# python3 + pdfplumber 用于 lopdf 失败时的真实坐标兜底提取
RUN apt-get update && apt-get install -y --no-install-recommends \
    ca-certificates libssl3 libgomp1 \
    libreoffice-core libreoffice-writer fonts-noto-cjk \
    python3 python3-pip \
    && pip3 install --break-system-packages --no-cache-dir pdfplumber \
    && rm -rf /var/lib/apt/lists/*

# 专用非 root 系统用户：供 docx_convert_service 以非 root 身份运行 soffice（安全加固）
RUN useradd -r -s /usr/sbin/nologin -m -d /home/soffice soffice

WORKDIR /app

COPY --from=build /app/server-release /usr/local/bin/server

# 规则库数据（conditions.yaml / catalog.yaml）—— 引擎按 CARGO_MANIFEST_DIR=/app 定位 /app/src/rules/data，必须拷入
COPY src/rules/data /app/src/rules/data

# Python pdfplumber 兜底脚本（extract_with_python 按 CARGO_MANIFEST_DIR=/app 定位 /app/scripts/pdf_extract.py）
COPY scripts /app/scripts

# 数据目录（中间产物、.env 等），宿主机挂卷
VOLUME /data

EXPOSE 3001

ENTRYPOINT ["server"]