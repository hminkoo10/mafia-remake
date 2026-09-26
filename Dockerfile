# ─── Stage 1: 프론트엔드 빌드 ───────────────────────────────
FROM node:20-alpine AS frontend
WORKDIR /app/activity
COPY activity/package*.json ./
RUN npm ci
COPY activity/ ./
RUN npm run build

# ─── Stage 1b: 카지노 웹 빌드 ───────────────────────────────
FROM node:20-alpine AS casino-frontend
WORKDIR /app/casino-web
COPY casino-web/package*.json ./
RUN npm ci
COPY casino-web/ ./
RUN npm run build

# ─── Stage 1c: 증권 사이트 빌드 ─────────────────────────────
FROM node:20-alpine AS stocks-frontend
WORKDIR /app/stocks-web
COPY stocks-web/package*.json ./
RUN npm ci
COPY stocks-web/ ./
RUN npm run build

# ─── Stage 2: Rust 빌드 ──────────────────────────────────────
FROM rust:1.88-slim AS builder
WORKDIR /app

RUN apt-get update && apt-get install -y \
    pkg-config libssl-dev ca-certificates \
    && rm -rf /var/lib/apt/lists/*

COPY . .
COPY --from=frontend /app/activity/dist ./activity/dist
COPY --from=casino-frontend /app/casino-web/dist ./casino-web/dist
COPY --from=stocks-frontend /app/stocks-web/dist ./stocks-web/dist

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
COPY --from=builder /app/config.example.json ./config.example.json
COPY --from=frontend /app/activity/dist ./activity/dist
COPY --from=casino-frontend /app/casino-web/dist ./casino-web/dist
COPY --from=stocks-frontend /app/stocks-web/dist ./stocks-web/dist

ENV ACTIVITY_STATIC_DIR=/app/activity/dist
ENV CASINO_STATIC_DIR=/app/casino-web/dist
ENV STOCKS_STATIC_DIR=/app/stocks-web/dist
ENV ACTIVITY_PORT=2053
ENV WEB_SETTINGS_PORT=8800
ENV WEB_SETTINGS_HOST=0.0.0.0

EXPOSE 2053

CMD ["./mafia"]
