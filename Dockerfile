# ===== Build Stage =====
FROM rust:1.91 AS builder
WORKDIR /app
RUN apt-get update && apt-get install -y cmake build-essential ca-certificates && rm -rf /var/lib/apt/lists/*
COPY . .
RUN cargo build --release

# ===== Run Stage =====
FROM ubuntu:24.04
RUN apt-get update && apt-get install -y ca-certificates && rm -rf /var/lib/apt/lists/*
COPY --from=builder /app/target/release/miner /usr/local/bin/miner
ENTRYPOINT ["/usr/local/bin/miner"]
CMD []
