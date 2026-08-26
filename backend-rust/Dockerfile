# ── Rust 引擎多阶段构建：编译 → 精简运行 ──
# 必须用 bookworm 变体：运行阶段是 debian:bookworm-slim(glibc 2.36)，
# 若用默认 rust:1(基于 Trixie, glibc 2.39)编译，产物会报 GLIBC_2.38/2.39 not found
FROM rust:1-bookworm AS build

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

# 先复制依赖清单，利用 Docker 层缓存（依赖没变就不重编）
COPY Cargo.toml Cargo.lock build.rs rust-toolchain.toml ./
COPY src ./src

# 编译 HTTP 服务入口（首次会下载并编译大量 crate，耗时较长）
RUN cargo build --release --bin server

# ── 运行阶段 ──
FROM debian:bookworm-slim

# 运行时依赖：ca-certificates（HTTPS）+ libssl3 + libgomp1（ONNX 动态库的 OpenMP）
RUN apt-get update && apt-get install -y --no-install-recommends \
    ca-certificates libssl3 libgomp1 \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /app

COPY --from=build /app/target/release/server /usr/local/bin/server

# 规则库数据（conditions.yaml / catalog.yaml）—— 引擎按 CARGO_MANIFEST_DIR=/app 定位 /app/src/rules/data，必须拷入
COPY src/rules/data /app/src/rules/data

# 数据目录（中间产物、.env 等），宿主机挂卷
VOLUME /data

EXPOSE 3001

ENTRYPOINT ["server"]