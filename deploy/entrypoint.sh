#!/bin/bash
set -Eeuo pipefail
umask 007

readonly V2S_UID="${VPNGATE2SOCKS_UNPRIVILEGED_UID:-10001}"
readonly V2S_GID="${VPNGATE2SOCKS_UNPRIVILEGED_GID:-10001}"
readonly V2S_RUNTIME="${VPNGATE2SOCKS_RUNTIME_DIR:-/run/vpngate2socks}"

install -d -m 0750 -o 0 -g "${V2S_GID}" "${V2S_RUNTIME}"
install -d -m 0750 -o "${V2S_UID}" -g "${V2S_GID}" /var/lib/vpngate2socks

setpriv --reuid=0 --regid="${V2S_GID}" --clear-groups \
    /usr/local/bin/vpngate2socks netd &
readonly V2S_NETD_PID=$!

terminate() {
    if [[ -n "${V2S_APP_PID:-}" ]] && kill -0 "${V2S_APP_PID}" 2>/dev/null; then
        kill -TERM "${V2S_APP_PID}" 2>/dev/null || true
    fi
    if kill -0 "${V2S_NETD_PID}" 2>/dev/null; then
        kill -TERM "${V2S_NETD_PID}" 2>/dev/null || true
    fi
}
trap terminate TERM INT EXIT

for _attempt in $(seq 1 100); do
    if [[ -S "${V2S_RUNTIME}/netd.sock" ]]; then
        break
    fi
    if ! kill -0 "${V2S_NETD_PID}" 2>/dev/null; then
        wait "${V2S_NETD_PID}"
        exit $?
    fi
    sleep 0.05
done
if [[ ! -S "${V2S_RUNTIME}/netd.sock" ]]; then
    echo "netd did not create its private socket" >&2
    exit 1
fi

setpriv --reuid="${V2S_UID}" --regid="${V2S_GID}" --clear-groups \
    --inh-caps=-all --ambient-caps=-all --bounding-set=-all --no-new-privs \
    /usr/local/bin/vpngate2socks serve &
V2S_APP_PID=$!

set +e
wait -n "${V2S_NETD_PID}" "${V2S_APP_PID}"
V2S_STATUS=$?
set -e
terminate
wait "${V2S_NETD_PID}" 2>/dev/null || true
wait "${V2S_APP_PID}" 2>/dev/null || true
trap - EXIT
exit "${V2S_STATUS}"
