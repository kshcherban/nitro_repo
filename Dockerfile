############################
# Frontend build stage
############################
FROM node:20-bookworm AS frontend-builder
WORKDIR /app/site

COPY site/package*.json ./
RUN npm install
COPY site .
RUN npm run build

############################
# Rust build stage
############################
FROM rust:1.78-bookworm AS rust-builder
WORKDIR /app

RUN apt-get update \
    && apt-get install -y --no-install-recommends pkg-config libssl-dev \
    && rm -rf /var/lib/apt/lists/*

COPY Cargo.lock Cargo.toml ./
COPY crates crates
COPY nitro_repo nitro_repo
COPY docs docs
COPY site site
COPY --from=frontend-builder /app/site/dist ./site/dist

ENV FRONTEND_DIST=/app/site/dist

RUN cargo build --release --features frontend

############################
# Runtime stage
############################
FROM debian:bookworm-slim AS runtime

RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates libssl3 \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /app
COPY --from=rust-builder /app/target/release/nitro_repo ./nitro_repo

EXPOSE 6742
VOLUME ["/data"]

ENV RUST_LOG=info

ENTRYPOINT ["./nitro_repo"]
CMD []
