# syntax=docker/dockerfile:1
FROM rust:1.99.0-trixie AS builder
WORKDIR /app
COPY Cargo.toml Cargo.lock ./
COPY crates ./crates
COPY docs ./docs
COPY data/cache/mappings/rules.json ./data/cache/mappings/rules.json
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/usr/local/cargo/git \
    --mount=type=cache,target=/app/target \
    cargo build --locked --release -j 2 -p atlas-server \
    && install -Dm755 target/release/atlas-server /out/atlas-server

FROM debian:trixie-slim AS runtime
RUN apt-get update && apt-get install -y --no-install-recommends ca-certificates curl \
    && rm -rf /var/lib/apt/lists/* \
    && useradd --uid 10001 --create-home atlas
COPY --from=builder /out/atlas-server /usr/local/bin/atlas-server
WORKDIR /app
ENV RARE_ATLAS_DATA=/data
USER atlas
EXPOSE 8000
ENTRYPOINT ["atlas-server"]
CMD ["serve", "--addr", "0.0.0.0:8000"]
