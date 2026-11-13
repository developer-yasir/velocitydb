# ==============================================================================
# Multi-Stage Dockerfile for VelocityDB
# Powered by Linux io_uring and Thread-per-Core architecture
# ==============================================================================

# Build Stage
FROM rust:bookworm AS builder

WORKDIR /usr/src/velocitydb

# Copy manifest and dependencies
COPY Cargo.toml Cargo.lock ./

# Copy source code and tests
COPY src ./src
COPY tests ./tests

# Build optimized production binary
RUN cargo build --release --bin velocitydb

# Strip debug symbols to reduce binary size
RUN strip /usr/src/velocitydb/target/release/velocitydb

# Runtime Stage
FROM debian:bookworm-slim

RUN apt-get update && apt-get install -y --no-install-recommends \
    ca-certificates \
    libssl3 \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /opt/velocitydb

# Copy binary from builder
COPY --from=builder /usr/src/velocitydb/target/release/velocitydb /opt/velocitydb/velocitydb

# Client Port (6379), Cluster Bus Port (16379), and Studio HTTP/WebSocket Port (8080)
EXPOSE 6379 16379 8080

ENTRYPOINT ["/opt/velocitydb/velocitydb"]
CMD ["0.0.0.0:6379"]
