# Multi-stage Dockerfile for code-rag workspace
# Supports two targets:
#   - chat (default): The query server
#   - ingest: The ingestion CLI (code-rag-ingest)
#
# Usage:
#   docker build --target chat -t code-rag .
#   docker build --target ingest -t code-rag-ingest .

# Builder stage: compile the Rust application
FROM rust:1.95-slim AS builder

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
COPY code-rag/crates/code-rag-core/Cargo.toml ./crates/code-rag-core/
COPY code-rag/crates/code-rag-llm/Cargo.toml ./crates/code-rag-llm/
COPY code-rag/crates/code-rag-ingest/Cargo.toml ./crates/code-rag-ingest/
COPY code-rag/crates/code-raptor/Cargo.toml ./crates/code-raptor/
COPY code-rag/crates/code-rag-ui/Cargo.toml ./crates/code-rag-ui/
COPY code-rag/crates/code-rag-mcp/Cargo.toml ./crates/code-rag-mcp/

# caravan-rpc resolves from crates.io as a workspace dep — no sibling
# checkout, no [patch.crates-io] override. The earlier `COPY caravan/rpc/rust/...`
# shim was retired at M9-cloud close (2026-05-27) when 0.1.1 published.

# Create dummy sources for dependency caching
RUN mkdir -p src/bin crates/code-rag-types/src crates/code-rag-engine/src crates/code-rag-store/src crates/code-rag-core/src crates/code-rag-llm/src crates/code-rag-ingest/src crates/code-raptor/src crates/code-rag-ui/src crates/code-rag-mcp/src && \
    echo "fn main() {println!(\"dummy\")}" > src/main.rs && \
    echo "fn main() {}" > src/bin/harness.rs && \
    echo "// dummy" > crates/code-rag-types/src/lib.rs && \
    echo "// dummy" > crates/code-rag-engine/src/lib.rs && \
    echo "// dummy" > crates/code-rag-store/src/lib.rs && \
    echo "// dummy" > crates/code-rag-core/src/lib.rs && \
    echo "// dummy" > crates/code-rag-llm/src/lib.rs && \
    echo "fn main() {}" > crates/code-rag-ingest/src/main.rs && \
    echo "// dummy" > crates/code-rag-ingest/src/lib.rs && \
    echo "// dummy" > crates/code-raptor/src/lib.rs && \
    echo "fn main() {}" > crates/code-rag-ui/src/main.rs && \
    echo "fn main() {}" > crates/code-rag-mcp/src/main.rs

# Build server crates to cache dependencies — exclude code-rag-ui (WASM
# target, built via trunk below) and code-rag-mcp (standalone single-binary
# release artifact, not used by the chat / raptor compose images).
RUN cargo build --release --workspace --exclude code-rag-ui --exclude code-rag-mcp && \
    rm -rf src crates/code-rag-types/src crates/code-rag-engine/src crates/code-rag-store/src crates/code-rag-core/src crates/code-rag-llm/src crates/code-rag-ingest/src crates/code-raptor/src crates/code-rag-ui/src crates/code-rag-mcp/src

# Copy actual source for all crates
COPY code-rag/src ./src
COPY code-rag/crates/code-rag-types/src ./crates/code-rag-types/src
COPY code-rag/crates/code-rag-engine/src ./crates/code-rag-engine/src
COPY code-rag/crates/code-rag-store/src ./crates/code-rag-store/src
COPY code-rag/crates/code-rag-core/src ./crates/code-rag-core/src
COPY code-rag/crates/code-rag-llm/src ./crates/code-rag-llm/src
COPY code-rag/crates/code-rag-ingest/src ./crates/code-rag-ingest/src
COPY code-rag/crates/code-raptor/src ./crates/code-raptor/src
COPY code-rag/crates/code-rag-ui/src ./crates/code-rag-ui/src
COPY code-rag/crates/code-rag-ui/index.html ./crates/code-rag-ui/index.html
COPY code-rag/crates/code-rag-ui/style ./crates/code-rag-ui/style
COPY code-rag/crates/code-rag-ui/public ./crates/code-rag-ui/public
COPY code-rag/crates/code-rag-ui/static ./crates/code-rag-ui/static

# Rebuild server with real source
RUN touch src/main.rs \
          src/bin/harness.rs \
          crates/code-rag-types/src/lib.rs \
          crates/code-rag-engine/src/lib.rs \
          crates/code-rag-store/src/lib.rs \
          crates/code-rag-core/src/lib.rs \
          crates/code-rag-llm/src/lib.rs \
          crates/code-rag-ingest/src/lib.rs \
          crates/code-rag-ingest/src/main.rs \
          crates/code-raptor/src/lib.rs && \
    cargo build --release --workspace --exclude code-rag-ui --exclude code-rag-mcp

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
# Target: ingest - The ingestion CLI (code-rag-ingest)
# ============================================================
FROM gcr.io/distroless/cc-debian13 AS ingest

WORKDIR /app

COPY --from=builder /build/target/release/code-rag-ingest ./code-rag-ingest

ENV DB_PATH=/app/data/portfolio.lance
ENV RUST_LOG=info
ENV HOME=/tmp
ENV FASTEMBED_CACHE_DIR=/tmp/.cache

# No default CMD - user provides subcommand (ingest, status)
