#!/bin/bash
set -Eeuo pipefail

readonly V2S_IMAGE="${VPNGATE2SOCKS_TEST_IMAGE:-localhost/vpngate2socks:test}"

podman build --tag "${V2S_IMAGE}" --file Containerfile .
podman run --rm \
    --device /dev/net/tun:/dev/net/tun \
    --cap-add NET_ADMIN \
    --cap-add SYS_ADMIN \
    --entrypoint /bin/bash \
    "${V2S_IMAGE}" -Eeuo pipefail -c '
cleanup() {
    ip netns delete v2s-smoke-a 2>/dev/null || true
    ip netns delete v2s-smoke-b 2>/dev/null || true
}
trap cleanup EXIT
for namespace in v2s-smoke-a v2s-smoke-b; do
    ip netns add "${namespace}"
    nsenter --net="/run/netns/${namespace}" unshare --mount --propagation private /bin/sh -eu -c "
        mount -t proc proc /proc
        echo 1 > /proc/sys/net/ipv6/conf/all/disable_ipv6
        echo 1 > /proc/sys/net/ipv6/conf/default/disable_ipv6
        echo 1 > /proc/sys/net/ipv6/conf/lo/disable_ipv6
    "
    nsenter --net="/run/netns/${namespace}" ip link set lo up
    nsenter --net="/run/netns/${namespace}" ip tuntap add dev tun0 mode tun
    nsenter --net="/run/netns/${namespace}" ip link set tun0 up
    nsenter --net="/run/netns/${namespace}" nft add table inet leak_guard
    nsenter --net="/run/netns/${namespace}" nft "add chain inet leak_guard output { type filter hook output priority filter; policy drop; }"
    nsenter --net="/run/netns/${namespace}" ip link show dev tun0
done
'
