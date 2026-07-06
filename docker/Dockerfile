# Multi-stage build for Rwiki
# Build order: backend first, then frontend (which depends on backend for API generation)

# Stage 1: Shared Rust build environment
FROM rust:1.96-bookworm AS backend-chef
WORKDIR /app

RUN apt-get update && apt-get install -y \
    cmake \
    libssl-dev \
    pkg-config && \
    rm -rf /var/lib/apt/lists/* && \
    cargo install cargo-chef --locked

# Stage 2: Analyze backend dependency graph
FROM backend-chef AS backend-planner
COPY backend/ ./
RUN cargo chef prepare --recipe-path recipe.json

# Stage 3: Build backend
FROM backend-chef AS backend-builder
COPY --from=backend-planner /app/recipe.json recipe.json
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/usr/local/cargo/git \
    cargo chef cook --release --bin app --recipe-path recipe.json

COPY backend/ ./

RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/usr/local/cargo/git \
    cargo build --release --bin app

# Stage 4: Build frontend (depends on backend for API generation)
FROM node:20-bookworm AS frontend-builder
WORKDIR /frontend

RUN apt-get update && apt-get install -y openssl && rm -rf /var/lib/apt/lists/*

COPY frontend/package*.json ./
RUN npm ci --legacy-peer-deps

COPY --from=backend-builder /app/target/release/app /tmp/rwiki-app

COPY frontend/ ./

RUN (/tmp/rwiki-app --export-openapi api.json && npx openapi-ts || echo "Warning: API generation failed, using existing types") && \
    npm run build && \
    npm run build:widget

# Stage 5: Final runtime image
FROM debian:bookworm-slim

RUN apt-get update && apt-get install -y --no-install-recommends \
    ca-certificates \
    libgcc-s1 \
    tzdata \
    wget \
    && rm -rf /var/lib/apt/lists/* \
    && addgroup --gid 1000 rwiki \
    && adduser --disabled-password --gecos "" --uid 1000 --gid 1000 rwiki

WORKDIR /app

COPY --from=backend-builder /app/target/release/app /app/rwiki-app
# Web SPA (index.html + assets/*) + widget (rwiki-chat.js) 全部进 /app/static/
COPY --from=frontend-builder /frontend/dist/ /app/static/

RUN chown -R rwiki:rwiki /app && \
    chmod +x /app/rwiki-app

# Data volume for SQLite database persistence
VOLUME /app/data

USER rwiki

EXPOSE 8080

HEALTHCHECK --interval=30s --timeout=3s --start-period=10s --retries=3 \
    CMD wget --no-verbose --tries=1 --spider http://localhost:8080/health || exit 1

ENV APP_CONFIG=/app/config.toml

CMD ["./rwiki-app"]
