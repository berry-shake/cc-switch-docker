# syntax=docker/dockerfile:1.7
#
# cc-switch-server：无界面的 AI CLI 用量统计服务（Web 面板 + HTTP API）
#
#   docker build -t cc-switch-server .
#   docker compose up -d        # 见 docker-compose.yml
#
# 三个阶段：Web 面板（pnpm build:web）→ Rust 服务端（--features web-server）→ 运行时镜像。

ARG NODE_VERSION=22
ARG RUST_VERSION=1.95
ARG DEBIAN_RELEASE=bookworm

# ── 1. Web 面板 ────────────────────────────────────────────────
FROM node:${NODE_VERSION}-${DEBIAN_RELEASE}-slim AS web
WORKDIR /src
RUN corepack enable
COPY package.json pnpm-lock.yaml pnpm-workspace.yaml ./
RUN --mount=type=cache,id=pnpm-store,target=/root/.local/share/pnpm/store \
    corepack install && pnpm install --frozen-lockfile
COPY . .
RUN pnpm build:web

# ── 2. Rust 服务端 ─────────────────────────────────────────────
FROM rust:${RUST_VERSION}-${DEBIAN_RELEASE} AS server
# 服务端和桌面版是同一个 crate，编译期仍需要 GTK / WebKit 的开发包（运行时不加载窗口）
RUN apt-get update && apt-get install -y --no-install-recommends \
        build-essential pkg-config libssl-dev \
        libgtk-3-dev librsvg2-dev libayatana-appindicator3-dev \
        libwebkit2gtk-4.1-dev libsoup-3.0-dev \
    && rm -rf /var/lib/apt/lists/*
WORKDIR /src
COPY src-tauri ./src-tauri
COPY rust-toolchain.toml ./
# tauri::generate_context! 在编译期要求 frontendDist（../dist）存在；服务端不用桌面版界面，放个占位页
RUN mkdir -p dist && echo '<!doctype html><title>CC Switch</title>' > dist/index.html
WORKDIR /src/src-tauri
RUN --mount=type=cache,id=cargo-registry,target=/usr/local/cargo/registry \
    --mount=type=cache,id=cargo-git,target=/usr/local/cargo/git \
    --mount=type=cache,id=cc-switch-target,target=/src/src-tauri/target \
    cargo build --release --locked --features web-server --bin cc-switch-server \
    && install -Dm755 target/release/cc-switch-server /out/cc-switch-server

# ── 3. 运行时 ──────────────────────────────────────────────────
FROM debian:${DEBIAN_RELEASE}-slim
# 二进制动态链接了 GTK / WebKit（与桌面版共用 crate），运行时库必须在；tzdata 让按天统计跟随 TZ
RUN apt-get update && apt-get install -y --no-install-recommends \
        ca-certificates tzdata libssl3 \
        libgtk-3-0 \
        libwebkit2gtk-4.1-0 libsoup-3.0-0 \
    && rm -rf /var/lib/apt/lists/*

COPY --from=server /out/cc-switch-server /app/cc-switch-server
COPY --from=web /src/dist-web /app/web

ENV HOME=/root \
    TZ=Asia/Shanghai \
    CC_SWITCH_WEB_LISTEN=0.0.0.0:8765 \
    CC_SWITCH_WEB_DIST=/app/web

# 数据库、设置、备份都在这里；会话目录（~/.claude、~/.codex …）按需只读挂载到 /root 下
VOLUME ["/root/.cc-switch"]
EXPOSE 8765
HEALTHCHECK --interval=30s --timeout=5s --start-period=20s --retries=3 \
    CMD ["/app/cc-switch-server", "--healthcheck"]

ENTRYPOINT ["/app/cc-switch-server"]
