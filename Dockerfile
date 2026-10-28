# syntax=docker/dockerfile:1.7@sha256:a57df69d0ea827fb7266491f2813635de6f17269be881f696fbfdf2d83dda33e

FROM node:26-bookworm-slim@sha256:86f07bc9c5dce4578cf37e5a418b7bfc7f817cda25cde66e2b66e95ed86c4567 AS web-builder
WORKDIR /src/web
ARG SENLUN_BUILD_GIT_HASH=unknown
ENV SENLUN_BUILD_GIT_HASH=${SENLUN_BUILD_GIT_HASH}
COPY web/package.json web/package-lock.json ./
RUN npm ci
COPY web/ ./
RUN npm run build

FROM rust:1.98-bookworm@sha256:9a73a5088750b4c95158ab26629c854c3d6fc4b173cb7bc8079ad252d8ed7bfa AS rust-builder
WORKDIR /src
ARG SENLUN_BUILD_GIT_HASH=unknown
ENV SENLUN_BUILD_GIT_HASH=${SENLUN_BUILD_GIT_HASH} \
    RUSTUP_TOOLCHAIN=1.98.0
COPY Cargo.toml Cargo.lock rust-toolchain.toml ./
COPY crates/ crates/
COPY --from=web-builder /src/web/dist web/dist
RUN cargo build --locked --release -p senlun-service -p senlun-agent

FROM debian:bookworm-slim@sha256:7c7b2c966bc9ee8cedfeef67e0e279108992c77681fa595db4a9d65c06ccc587 AS runtime
ARG SENLUN_BUILD_GIT_HASH=unknown
LABEL org.opencontainers.image.source="https://github.com/petauron/senlun" \
      org.opencontainers.image.description="Self-hosted node monitoring with an outbound-only Agent" \
      org.opencontainers.image.licenses="Apache-2.0" \
      org.opencontainers.image.revision="${SENLUN_BUILD_GIT_HASH}"
RUN apt-get update \
    && apt-get install --yes --no-install-recommends ca-certificates \
    && apt-get clean \
    && rm -rf /var/lib/apt/lists/* \
    && groupadd --gid 65532 senlun \
    && useradd --uid 65532 --gid senlun --system --home-dir /var/lib/senlun --shell /usr/sbin/nologin senlun \
    && install --directory --owner senlun --group senlun --mode 0700 /var/lib/senlun
COPY --from=rust-builder /src/target/release/senlun-service /usr/local/bin/senlun-service
COPY --from=rust-builder /src/target/release/senlun-agent /usr/local/bin/senlun-agent
COPY LICENSE NOTICE RUST_THIRD_PARTY_LICENSES.html RUST_STDLIB_LICENSES.html /usr/share/doc/senlun/
COPY web/THIRD_PARTY_NOTICES.md web/LICENSE.* /usr/share/doc/senlun/web/
COPY --from=web-builder /src/web/dist/THIRD_PARTY_LICENSES.md /usr/share/doc/senlun/web/
USER 65532:65532
WORKDIR /var/lib/senlun
ENV SENLUN_LISTEN=0.0.0.0:8080 \
    SENLUN_ALLOW_PUBLIC_LISTEN=true \
    SENLUN_DATABASE_PATH=/var/lib/senlun/senlun-store/senlun.db
VOLUME ["/var/lib/senlun"]
EXPOSE 8080
STOPSIGNAL SIGTERM
ENTRYPOINT ["/usr/local/bin/senlun-service"]
CMD ["migrate-and-serve"]
