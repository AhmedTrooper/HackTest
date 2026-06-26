# Build Stage
FROM rust:1.85-slim AS builder
WORKDIR /usr/src/app
COPY . .
RUN cargo build --release

# Execution Stage
FROM debian:bookworm-slim
WORKDIR /usr/app
RUN apt-get update && apt-get install -y ca-certificates && rm -rf /var/lib/apt/lists/*
COPY --from=builder /usr/src/app/target/release/api ./api

# Expose port and configure environment
EXPOSE 8080
ENV PORT=8080
ENV RUST_LOG=info

CMD ["./api"]
