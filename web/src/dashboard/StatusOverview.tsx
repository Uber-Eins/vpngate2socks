import { zhCN as t } from "../i18n";
import type { StatusSnapshot } from "../types";

export function StatusOverview({ status }: { status: StatusSnapshot | undefined }) {
  const connection = status?.connection;
  const connectionLabel = connection?.state === "connected"
    ? t.connected
    : connection?.state === "connecting"
      ? t.connecting
      : connection?.state === "failed"
        ? t.failed
        : t.disconnected;
  const connectionTone = status?.proxyReady === true ? "online" : connection?.state === "failed" ? "danger" : "idle";
  const upstreamLabel = status === undefined
    ? "—"
    : status.upstreamState === "ready"
      ? t.upstreamReady
      : status.upstreamState === "checking"
        ? "检查中"
        : status.upstreamState === "authenticationFailed"
          ? "认证失败"
          : status.upstreamState === "netdUnavailable"
            ? "netd 离线"
            : "无法访问";
  const activeNode = connection?.state === "connected" || connection?.state === "connecting"
    ? connection.nodeId.slice(0, 10)
    : undefined;

  return (
    <aside className="status-overview" aria-label="服务状态">
      <div className="status-overview__primary">
        <span className={`status-dot status-dot--${connectionTone}`} aria-hidden="true" />
        <div>
          <span>SOCKS5 · {status?.proxyReady === true ? t.ready : t.notReady}</span>
          <strong>{connectionLabel}</strong>
          {activeNode !== undefined && <code>{activeNode}…</code>}
        </div>
      </div>
      <div className={status !== undefined && status.upstreamState !== "ready" ? "status-metric status-metric--danger" : "status-metric"}>
        <span>{t.upstream}</span>
        <strong>{upstreamLabel}</strong>
      </div>
      <div className="status-metric">
        <span>{t.accepted}</span>
        <strong>{status?.lastRefresh?.accepted ?? "—"}</strong>
      </div>
      <div className="status-metric">
        <span>{t.automaticTesting}</span>
        <strong>{status === undefined ? "—" : `${status.runningTests} / ${status.queuedTests}`}</strong>
        <small>{t.runningCount} / {t.queuedCount}</small>
      </div>
      {connection?.state === "failed" && (
        <p className="status-overview__error">{connection.message}</p>
      )}
    </aside>
  );
}
