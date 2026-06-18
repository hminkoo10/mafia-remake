# ─── Stage 1: 프론트엔드 빌드 ───────────────────────────────
FROM node:20-alpine AS frontend
WORKDIR /app/activity
COPY activity/package*.json ./
RUN npm ci
COPY activity/ ./
RUN npm run build

# ─── Stage 2: Rust 빌드 ──────────────────────────────────────
FROM rust:1.82-slim AS builder
WORKDIR /app

RUN apt-get update && apt-get install -y \
    pkg-config libssl-dev ca-certificates \
    && rm -rf /var/lib/apt/lists/*

COPY . .
COPY --from=frontend /app/activity/dist ./activity/dist

# sccache/Windows 설정 무시, Linux 빌드용 jobs 수 조정
ENV RUSTC_WRAPPER=""
ENV CARGO_BUILD_JOBS=4

RUN cargo build --release --bin mafia

# ─── Stage 3: 최종 이미지 ────────────────────────────────────
FROM debian:bookworm-slim
WORKDIR /app

RUN apt-get update && apt-get install -y \
    ca-certificates libssl3 \
    && rm -rf /var/lib/apt/lists/*

COPY --from=builder /app/target/release/mafia ./mafia
COPY --from=frontend /app/activity/dist ./activity/dist

ENV ACTIVITY_STATIC_DIR=/app/activity/dist
ENV ACTIVITY_PORT=8802
ENV WEB_SETTINGS_PORT=8800
ENV WEB_SETTINGS_HOST=0.0.0.0

EXPOSE 8802

CMD ["./mafia"]
