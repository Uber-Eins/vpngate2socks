# vpngate2socks

`vpngate2socks` 是一个 Linux 优先的本地 SOCKS5 控制台。它通过指定的上游 SOCKS5 获取 VPN Gate 节点，为活动连接和每次 IPPure 测试创建独立 network namespace，并在 VPN 或上游失败时闭锁出口。

```text
浏览器 → 127.0.0.1:1080 → 活动 worker → tun0/OpenVPN → 上游 SOCKS5 → VPN Gate → 互联网
```

## 快速启动（rootless Podman）

主机需要 Podman 6+、可用的 `/dev/net/tun`，以及一个可从容器访问的 SOCKS5 上游。上游可以写成 IPv4 地址，也可以写成 `host.containers.internal:端口` 这类 ASCII 主机名；IPv6 上游仍不在 v1 支持范围内。主机上的 mihomo 等代理必须监听容器可访问的端口。

```bash
cp .env.example .env
# 编辑 .env，至少设置 VPNGATE2SOCKS_UPSTREAM
podman compose up --build
```

WebUI 位于 `http://127.0.0.1:8080`，SOCKS5 位于 `127.0.0.1:1080`。Compose 只把端口发布到主机回环地址；容器内的 `0.0.0.0` 绑定由 `VPNGATE2SOCKS_CONTAINER_BIND=true` 显式授权，不能用于直接暴露容器 IP。

### Quadlet

仓库中的 `v2s.container` 可直接用于 rootless Quadlet，默认拉取 `ghcr.io/uber-eins/vpngate2socks:latest`，并将 WebUI/SOCKS5 分别发布到 `127.0.0.1:28080` 与 `127.0.0.1:21080`：

```bash
install -Dm644 v2s.container ~/.config/containers/systemd/v2s.container
# 按需编辑 VPNGATE2SOCKS_UPSTREAM
systemctl --user daemon-reload
systemctl --user enable --now v2s.service
```

配置使用只读根文件系统、持久化 named volume、所需的最小 capability 集合及 `/dev/net/tun`。如需上游认证或 LAN/TLS 配置，可通过 `v2s.container.d/*.conf` drop-in 增加 `Environment=` 或 `Secret=`。

推荐用 Podman secret 提供密码，而不是把密码放进环境变量：

```bash
printf '%s' 'upstream-password' | podman secret create v2s-upstream-password -
podman run --rm \
  --cap-drop ALL \
  --cap-add CHOWN --cap-add DAC_OVERRIDE --cap-add FOWNER --cap-add KILL \
  --cap-add NET_ADMIN --cap-add SETGID --cap-add SETPCAP --cap-add SETUID --cap-add SYS_ADMIN \
  --security-opt no-new-privileges --device /dev/net/tun \
  --sysctl net.ipv4.ip_forward=1 \
  --sysctl net.ipv6.conf.all.disable_ipv6=1 \
  --sysctl net.ipv6.conf.default.disable_ipv6=1 \
  --read-only --tmpfs /run:size=16m,mode=0755 --tmpfs /tmp:size=16m,mode=1777 \
  --volume vpngate2socks-data:/var/lib/vpngate2socks \
  --secret v2s-upstream-password,type=mount,target=upstream-password \
  -e VPNGATE2SOCKS_UPSTREAM=host.containers.internal:1080 \
  -e VPNGATE2SOCKS_UPSTREAM_USER=user \
  -e VPNGATE2SOCKS_UPSTREAM_PASSWORD_FILE=/run/secrets/upstream-password \
  -p 127.0.0.1:8080:8080 -p 127.0.0.1:1080:1080 \
  localhost/vpngate2socks:latest
```

## 安全模型

- 控制面以 UID/GID `10001` 运行，并清除 bounding、inheritable 与 ambient capabilities，同时启用 `no_new_privs`。
- OpenVPN 以独立 UID `10002` 运行，bounding、inheritable 与 ambient 集合中仅保留 `NET_ADMIN`；其私有目录不允许控制面 UID 读取或修改。
- 只有 `netd` 保留容器内的 `NET_ADMIN`/`SYS_ADMIN`。命令通过权限为 `0660`、位于 `0750` 目录中的类型化 Unix socket 传递。
- 每个 worker 拥有独立 netns、veth、`tun0`、OpenVPN management socket 和 Unix SOCKS socket。
- netd 使用 `nsenter` 进入子 netns，并在短生命周期 mount namespace 中挂载可写 procfs 来关闭 IPv6；这避开 rootless 容器中 `ip netns exec` 重挂载 `/sys` 的限制。
- 上游主机名只由 netd 在启动时解析一次，并固定为一个 IPv4 地址；该地址通过类型化 `Pong` 响应交给控制面。nftables、OpenVPN、VPN Gate 请求和上游健康探测因此始终使用同一个地址，不会因重复解析而绕过闭锁。
- worker 的 nftables 输出策略默认拒绝：veth 只允许访问配置的上游 IPv4/端口，业务流量只允许经 `tun0`。
- worker 内置有界的 IPv4 DNS 客户端，固定查询 `1.1.1.1` 和 `8.8.8.8`；查询只有在 VPN 路由可用时才能经 `tun0` 发出，不读取或回落到容器 DNS。
- 根命名空间的 nftables 输出策略也默认拒绝，仅放行回环、已建立连接和指定上游。VPN Gate API 使用 `socks5h`，不会直连回退。
- 控制面每 5 秒完成一次有超时的 SOCKS5 握手和认证探测；上游不可达或认证失败时 `/readyz` 返回 `503`，状态页显示具体原因。
- 下载的 `.ovpn` 不会直接执行。解析器只接受 TCP、匹配 CSV IP 的 `remote`、内联 CA/证书/密钥和有限加密选项；脚本、插件、外部文件、管理接口、路由注入和代理覆盖均被拒绝。
- worker 每秒检查 `tun0`、覆盖完整 IPv4 空间的隧道路由、OpenVPN management 状态和私有 SOCKS socket。隧道或路由消失时活动 relay 立即清空，新 SOCKS 请求失败。

此保证只覆盖发送到本地 SOCKS5 的 TCP。浏览器必须启用 SOCKS 远端 DNS，并禁用 WebRTC、QUIC 或其他直连旁路。VPN Gate 节点由志愿者运营；始终使用 HTTPS，不要把出口节点视为可信网络。

## 节点与切换

- 节点每 10 分钟刷新，失败时保留旧快照并指数退避；也可从 WebUI 手动刷新。
- CSV 响应、行数和 Base64 profile 均有限制。坏行只计入拒绝统计，不会破坏其余快照。
- v1 仅使用 TCP OpenVPN。UDP-only 节点仍显示为不可用。
- 切换采用 make-before-break：新 worker 到达 OpenVPN `CONNECTED,SUCCESS` 后才原子替换 relay，旧 worker 排空 30 秒。新连接失败时旧节点保持活动。
- 可用节点没有持久化的 IPPure 结果时会自动进入有界测试队列；高评分、低 Ping 节点优先。失败记录视为一次已完成检测，避免故障节点无限重试，可从 WebUI 手动重新检测。
- 测试队列默认最多并行 3 个。每个测试使用临时 worker，经其 SOCKS5 以远端 DNS 请求 IPPure，不会修改活动 relay；手动与自动请求会按节点去重。

## 自动连接

- WebUI 可启用持久化自动连接策略，并按 VPN Gate 地区代码、IPPure 广播/原生分类及住宅属性筛选节点。
- 不限制 IPPure 属性时，尚未检测的节点也可参与选择；选择广播/原生或住宅条件后，只使用已有成功检测结果的节点。
- 匹配节点始终按带宽优先，评分与 Ping 用作稳定的同带宽排序条件。
- 活动隧道意外中断后会立即闭锁 SOCKS 出口，短期避开故障节点并以有界退避自动重连。手动连接或断开会关闭自动策略，避免违背显式操作。

## API

| 方法 | 路径 | 用途 |
|---|---|---|
| `GET` | `/api/v1/nodes` | 分页、搜索、排序和最近测试 |
| `POST` | `/api/v1/nodes/refresh` | 手动刷新 |
| `PUT` / `DELETE` | `/api/v1/connection` | 切换 / 断开 |
| `GET` / `PUT` | `/api/v1/auto-connection` | 读取 / 更新自动连接策略与地区选项 |
| `POST` | `/api/v1/nodes/{nodeId}/tests` | 排队隔离测试 |
| `GET` | `/api/v1/tests/{operationId}` | 测试状态 |
| `GET` | `/api/v1/status` | relay、队列、刷新和 helper 状态 |
| `GET` | `/api/v1/events` | SSE 事件 |
| `GET` | `/healthz`, `/readyz` | liveness / readiness |

SOCKS5 v1 支持 `CONNECT`、IPv4 和域名；拒绝 `BIND`、`UDP ASSOCIATE`、IPv6、回环、链路本地、私网、文档网段和解析到这些地址的域名。

## LAN 与 TLS

直接在 LAN 上监听必须设置 `VPNGATE2SOCKS_LAN_MODE=true`，并同时配置：

```text
VPNGATE2SOCKS_WEB_USER / VPNGATE2SOCKS_WEB_PASSWORD[_FILE]
VPNGATE2SOCKS_SOCKS_USER / VPNGATE2SOCKS_SOCKS_PASSWORD[_FILE]
```

WebUI 使用 HttpOnly/SameSite 会话 Cookie 与 CSRF header；SOCKS 使用 RFC 1929。设置 `VPNGATE2SOCKS_TLS_CERT` 和 `VPNGATE2SOCKS_TLS_KEY` 可启用内置 HTTPS。LAN 模式未启用 TLS 时，日志和 WebUI 会持续显示明文凭据警告。

## 本地开发与验证

```bash
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test --all-targets

npm --prefix web ci
npm --prefix web run typecheck
npm --prefix web test
npm --prefix web run build
```

rootless namespace/TUN/nftables 冒烟测试：

```bash
./scripts/podman-smoke.sh
```

运行非容器开发实例时需分别启动特权 helper 和控制面，并设置 `VPNGATE2SOCKS_UPSTREAM`、可写的 runtime/database 路径。`netd` 的主 GID 必须与 `VPNGATE2SOCKS_UNPRIVILEGED_GID` 相同，以便无特权控制面访问私有 socket。
