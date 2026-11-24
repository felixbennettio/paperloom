# Paperloom Cloud, built from the repository root:
#
#   docker build -f server/Dockerfile -t paperloom-server .
#
# Settings are environment variables; see docs/sync-protocol.md and
# server/compose.example.yml.

FROM rust:1-bookworm AS build
WORKDIR /src
COPY . .
RUN cargo build --release --locked --manifest-path server/Cargo.toml

FROM debian:bookworm-slim
RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates \
    && rm -rf /var/lib/apt/lists/*
COPY --from=build /src/server/target/release/paperloom-server /usr/local/bin/paperloom-server
RUN mkdir -p /var/lib/paperloom/blobs && chown 65534:65534 /var/lib/paperloom/blobs
USER 65534:65534
EXPOSE 8080
HEALTHCHECK CMD ["/bin/bash", "-c", "exec 3<>/dev/tcp/127.0.0.1/8080"]
ENTRYPOINT ["paperloom-server"]
