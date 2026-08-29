# syntax=docker/dockerfile:1.7

# The web bundle is architecture independent, so build it on the native build
# platform even when the final image targets another architecture.
FROM --platform=$BUILDPLATFORM node:26-alpine3.22 AS web-builder
WORKDIR /build/web
COPY web/package.json web/package-lock.json ./
RUN npm ci
COPY web/ ./
RUN npm run build

FROM rust:1.90-alpine3.22 AS rust-builder
RUN apk add --no-cache musl-dev openssl-dev pkgconf
WORKDIR /build
COPY Cargo.toml Cargo.lock ./
COPY migrations/ migrations/
COPY src/ src/
# Link against the musl libc and OpenSSL the runtime image already ships rather
# than statically bundling a second copy of each into the binary.
ENV RUSTFLAGS="-C target-feature=-crt-static"
RUN --mount=type=cache,target=/usr/local/cargo/registry,sharing=locked \
    --mount=type=cache,target=/build/target,sharing=locked \
    cargo build --locked --release \
    && install -D -m 0755 target/release/vpngate2socks /out/vpngate2socks

# Only nsenter and unshare are needed out of util-linux-misc, and both link
# against nothing but musl, so take the two binaries instead of the 5.9 MiB
# package.
FROM alpine:3.22 AS util-linux
RUN apk add --no-cache util-linux-misc

FROM alpine:3.22
# iproute2-minimal ships /sbin/ip; busybox already provides mount, install,
# seq and wget. /usr/lib/bash holds 2.4 MiB of loadable builtins nothing here
# ever calls.
RUN apk add --no-cache \
        bash \
        iproute2-minimal \
        libcrypto3 \
        libgcc \
        libssl3 \
        nftables \
        openvpn \
        setpriv \
        tini \
    && rm -rf /usr/lib/bash \
    && addgroup -S -g 10001 vpngate2socks \
    && adduser -S -D -H -u 10001 -G vpngate2socks -s /sbin/nologin vpngate2socks \
    && adduser -S -D -H -u 10002 -G vpngate2socks -s /sbin/nologin openvpn-worker \
    && install -d -m 0750 -o 10001 -g 10001 /var/lib/vpngate2socks \
    && install -d -m 0755 -o 0 -g 0 /opt/vpngate2socks/web

COPY --from=util-linux /usr/bin/nsenter /usr/bin/unshare /usr/bin/
# --chmod keeps the mode fix in the same layer; a trailing `RUN chmod` would
# store a second full copy of the binary in the image.
COPY --from=rust-builder --chmod=0755 /out/vpngate2socks /usr/local/bin/vpngate2socks
COPY --from=web-builder /build/web/dist/ /opt/vpngate2socks/web/
COPY --chmod=0755 deploy/entrypoint.sh /usr/local/bin/vpngate2socks-entrypoint

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
    CMD sh -c 'if [ -n "${VPNGATE2SOCKS_TLS_CERT:-}" ]; then wget -q -Y off -T 3 --no-check-certificate -O /dev/null https://127.0.0.1:8080/healthz; else wget -q -Y off -T 3 -O /dev/null http://127.0.0.1:8080/healthz; fi'
ENTRYPOINT ["/sbin/tini", "--", "/usr/local/bin/vpngate2socks-entrypoint"]
