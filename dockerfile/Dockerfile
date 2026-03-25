# Multi-stage Dockerfile for code-rag workspace
# Supports two targets:
#   - chat (default): The query server
#   - raptor: The ingestion CLI
#
# Usage:
#   docker build --target chat -t code-rag .
#   docker build --target raptor -t code-raptor .

# Builder stage: compile the Rust application
FROM rust:1.92-slim AS builder

# Install necessary build dependencies
RUN apt-get update && apt-get install -y \
    pkg-config \
    libssl-dev \
    ca-certificates \
    protobuf-compiler \
    g++ \
    && rm -rf /var/lib/apt/lists/*

# Install trunk and WASM target for frontend build
RUN cargo install trunk && \
    rustup target add wasm32-unknown-unknown

WORKDIR /build

# Copy workspace manifests
COPY code-rag/Cargo.toml code-rag/Cargo.lock ./
COPY code-rag/crates/code-rag-types/Cargo.toml ./crates/code-rag-types/
COPY code-rag/crates/code-rag-engine/Cargo.toml ./crates/code-rag-engine/
COPY code-rag/crates/code-rag-store/Cargo.toml ./crates/code-rag-store/
COPY code-rag/crates/code-raptor/Cargo.toml ./crates/code-raptor/
COPY code-rag/crates/code-rag-ui/Cargo.toml ./crates/code-rag-ui/

# Create dummy sources for dependency caching
RUN mkdir -p src crates/code-rag-types/src crates/code-rag-engine/src crates/code-rag-store/src crates/code-raptor/src crates/code-rag-ui/src && \
    echo "fn main() {println!(\"dummy\")}" > src/main.rs && \
    echo "// dummy" > crates/code-rag-types/src/lib.rs && \
    echo "// dummy" > crates/code-rag-engine/src/lib.rs && \
    echo "// dummy" > crates/code-rag-store/src/lib.rs && \
    echo "fn main() {}" > crates/code-raptor/src/main.rs && \
    echo "// dummy" > crates/code-raptor/src/lib.rs && \
    echo "fn main() {}" > crates/code-rag-ui/src/main.rs

# Build server crates to cache dependencies (exclude UI — different target)
RUN cargo build --release --workspace --exclude code-rag-ui && \
    rm -rf src crates/code-rag-types/src crates/code-rag-engine/src crates/code-rag-store/src crates/code-raptor/src crates/code-rag-ui/src

# Copy actual source for all crates
COPY code-rag/src ./src
COPY code-rag/crates/code-rag-types/src ./crates/code-rag-types/src
COPY code-rag/crates/code-rag-engine/src ./crates/code-rag-engine/src
COPY code-rag/crates/code-rag-store/src ./crates/code-rag-store/src
COPY code-rag/crates/code-raptor/src ./crates/code-raptor/src
COPY code-rag/crates/code-rag-ui/src ./crates/code-rag-ui/src
COPY code-rag/crates/code-rag-ui/index.html ./crates/code-rag-ui/index.html
COPY code-rag/crates/code-rag-ui/style ./crates/code-rag-ui/style
COPY code-rag/crates/code-rag-ui/public ./crates/code-rag-ui/public

# Rebuild server with real source
RUN touch src/main.rs \
          crates/code-rag-types/src/lib.rs \
          crates/code-rag-engine/src/lib.rs \
          crates/code-rag-store/src/lib.rs \
          crates/code-raptor/src/lib.rs \
          crates/code-raptor/src/main.rs && \
    cargo build --release --workspace --exclude code-rag-ui

# Build WASM frontend
RUN cd crates/code-rag-ui && trunk build --release

# ============================================================
# Target: chat - The query server (default)
# ============================================================
FROM gcr.io/distroless/cc-debian13:nonroot AS chat

WORKDIR /app

COPY --from=builder /build/target/release/code-rag-chat ./code-rag-chat
COPY --from=builder /build/crates/code-rag-ui/dist ./dist

ENV DB_PATH=/app/data/portfolio.lance
ENV UI_DIST=/app/dist
ENV RUST_LOG=info
ENV HOST=0.0.0.0
ENV PORT=3000
ENV HOME=/tmp
ENV FASTEMBED_CACHE_DIR=/tmp/.cache

EXPOSE 3000

CMD ["/app/code-rag-chat"]

# ============================================================
# Target: raptor - The ingestion CLI
# ============================================================
FROM gcr.io/distroless/cc-debian13 AS raptor

WORKDIR /app

COPY --from=builder /build/target/release/code-raptor ./code-raptor

ENV DB_PATH=/app/data/portfolio.lance
ENV RUST_LOG=info
ENV HOME=/tmp
ENV FASTEMBED_CACHE_DIR=/tmp/.cache

# No default CMD - user provides subcommand (ingest, status)
