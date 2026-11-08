# ==============================================================================
# Multi-Stage Dockerfile for VelocityDB
# Powered by Linux io_uring and Thread-per-Core architecture
# ==============================================================================

# Build Stage
FROM rust:1.82-bookworm AS builder

WORKDIR /usr/src/velocitydb

# Copy manifest and dependencies
COPY Cargo.toml Cargo.lock ./

# Copy source code and tests
COPY src ./src
COPY tests ./tests

# Build optimized production binary
RUN cargo build --release --bin velocitydb

# Runtime Stage
FROM debian:bookworm-slim

RUN apt-get update && apt-get install -y --no-install-recommends \
    ca-certificates \
    libssl3 \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /opt/velocitydb

# Copy binary from builder
COPY --from=builder /usr/src/velocitydb/target/release/velocitydb /opt/velocitydb/velocitydb

# Client Port (6379) and Cluster Bus Port (16379)
EXPOSE 6379 16379

ENTRYPOINT ["/opt/velocitydb/velocitydb"]
CMD ["0.0.0.0:6379"]
