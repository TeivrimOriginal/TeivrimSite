# Build: the toolchain stage is discarded so the runtime image carries no
# compiler, no registry cache and no source.
FROM rust:1.83-slim-bookworm AS build

# SQLite is compiled from source by the `bundled` feature, so a C toolchain and
# the amalgamation headers are needed. pkg-config is not required because
# OpenSSL is avoided entirely (reqwest uses rustls).
RUN apt-get update && apt-get install -y --no-install-recommends \
        build-essential \
        ca-certificates \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /src

# Dependencies first so a source-only change does not refetch the registry.
COPY Cargo.toml Cargo.lock ./
COPY .cargo ./.cargo
RUN mkdir -p src \
    && echo 'fn main() {}' > src/main.rs \
    && cargo build --release --locked \
    && rm -rf src

COPY src ./src
COPY frontend ./frontend
# `touch` is enough: cargo keys its cache on mtime, and the placeholder binary
# has to be replaced rather than appended to.
RUN touch src/main.rs && cargo build --release --locked

# ---------------------------------------------------------------- runtime

FROM debian:bookworm-slim AS runtime

RUN apt-get update && apt-get install -y --no-install-recommends \
        ca-certificates \
        curl \
    && rm -rf /var/lib/apt/lists/* \
    && useradd --system --create-home --uid 10001 anime

WORKDIR /app

COPY --from=build /src/target/release/anime_db /usr/local/bin/anime_db
COPY --from=build /src/frontend /app/frontend

# The catalogue is a rebuildable cache, so it lives on a volume rather than in
# the image layer.
RUN mkdir -p /app/data && chown -R anime:anime /app
VOLUME ["/app/data"]

USER anime

ENV BIND_ADDR=0.0.0.0 \
    PORT=8082 \
    DB_PATH=/app/data/AnimeData.db \
    FRONTEND_DIR=/app/frontend

EXPOSE 8082

HEALTHCHECK --interval=30s --timeout=5s --start-period=15s --retries=3 \
    CMD curl -fsS http://127.0.0.1:8082/healthz || exit 1

ENTRYPOINT ["anime_db"]
