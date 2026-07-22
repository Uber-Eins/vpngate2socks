# syntax=docker/dockerfile:1.7

FROM node:26-bookworm-slim AS web-builder
WORKDIR /build/web
COPY web/package.json web/package-lock.json ./
RUN npm ci
COPY web/ ./
RUN npm run build

FROM rust:1.90-bookworm AS rust-builder
RUN apt-get update \
    && apt-get install -y --no-install-recommends cmake clang libssl-dev pkg-config \
    && rm -rf /var/lib/apt/lists/*
WORKDIR /build
COPY Cargo.toml Cargo.lock ./
COPY migrations/ migrations/
COPY src/ src/
RUN cargo build --locked --release

FROM debian:bookworm-slim
RUN apt-get update \
    && apt-get install -y --no-install-recommends \
        ca-certificates curl iproute2 libssl3 nftables openvpn tini util-linux \
    && rm -rf /var/lib/apt/lists/* \
    && groupadd --gid 10001 vpngate2socks \
    && useradd --uid 10001 --gid 10001 --no-create-home --shell /usr/sbin/nologin vpngate2socks \
    && useradd --uid 10002 --gid 10001 --no-create-home --shell /usr/sbin/nologin openvpn-worker \
    && install -d -m 0750 -o 10001 -g 10001 /var/lib/vpngate2socks \
    && install -d -m 0755 -o 0 -g 0 /opt/vpngate2socks/web

COPY --from=rust-builder /build/target/release/vpngate2socks /usr/local/bin/vpngate2socks
COPY --from=web-builder /build/web/dist/ /opt/vpngate2socks/web/
COPY deploy/entrypoint.sh /usr/local/bin/vpngate2socks-entrypoint
RUN chmod 0755 /usr/local/bin/vpngate2socks /usr/local/bin/vpngate2socks-entrypoint

ENV VPNGATE2SOCKS_WEB_BIND=0.0.0.0:8080 \
    VPNGATE2SOCKS_SOCKS_BIND=0.0.0.0:1080 \
    VPNGATE2SOCKS_CONTAINER_BIND=true \
    VPNGATE2SOCKS_RUNTIME_DIR=/run/vpngate2socks \
    VPNGATE2SOCKS_DATABASE_URL=sqlite:///var/lib/vpngate2socks/state.db?mode=rwc \
    VPNGATE2SOCKS_WEB_DIST=/opt/vpngate2socks/web \
    VPNGATE2SOCKS_UNPRIVILEGED_UID=10001 \
    VPNGATE2SOCKS_UNPRIVILEGED_GID=10001 \
    VPNGATE2SOCKS_OPENVPN_UID=10002

VOLUME ["/var/lib/vpngate2socks"]
EXPOSE 8080 1080
HEALTHCHECK --interval=30s --timeout=3s --start-period=10s --retries=3 \
    CMD sh -c 'if [ -n "${VPNGATE2SOCKS_TLS_CERT:-}" ]; then curl --noproxy "*" -fk https://127.0.0.1:8080/healthz; else curl --noproxy "*" -f http://127.0.0.1:8080/healthz; fi'
ENTRYPOINT ["/usr/bin/tini", "--", "/usr/local/bin/vpngate2socks-entrypoint"]
