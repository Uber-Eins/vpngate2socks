import { zhCN as t } from "../i18n";
import type { StatusSnapshot } from "../types";
import { Dot, StatusPill } from "../ui/Badge";
import { Button } from "../ui/Button";
import { PowerIcon, RefreshIcon } from "../ui/Icon";
import { connectionLabel, connectionTone, upstreamLabel, upstreamTone } from "../utils/status";

export function TopBar({ title, status, live, busy, onRefresh, onDisconnect }: {
  title: string;
  status: StatusSnapshot | undefined;
  live: boolean;
  busy: string | undefined;
  onRefresh: () => void;
  onDisconnect: () => void;
}) {
  const upstreamDegraded = status !== undefined && status.upstreamState !== "ready";
  const activeTests = status === undefined ? 0 : status.runningTests + status.queuedTests;

  return (
    <header className="topbar">
      <span className="topbar__title">{title}</span>
      <div className="topbar__pills">
        <StatusPill dot={connectionTone(status)} label={t.proxy}>
          {connectionLabel(status?.connection)}
        </StatusPill>
        <StatusPill
          tone={upstreamDegraded ? "danger" : "default"}
          dot={upstreamTone(status?.upstreamState)}
          label={t.upstream}
        >
          {upstreamLabel(status?.upstreamState)}
        </StatusPill>
        {activeTests > 0 && (
          <StatusPill dot="busy" label={t.testQueue}>
            {status?.runningTests ?? 0} / {status?.queuedTests ?? 0}
          </StatusPill>
        )}
        <span
          className="status-pill"
          title={live ? t.liveConnected : t.liveReconnecting}
        >
          <Dot tone={live ? "online" : "busy"} />
          {live ? t.liveConnected : t.liveReconnecting}
        </span>
      </div>
      <div className="topbar__actions">
        <Button
          onClick={onRefresh}
          busy={busy === "refresh"}
          disabled={busy !== undefined}
        >
          {busy === "refresh" ? null : <RefreshIcon size={14} />}
          {busy === "refresh" ? t.refreshing : t.refresh}
        </Button>
        <Button
          variant="danger"
          onClick={onDisconnect}
          busy={busy === "disconnect"}
          disabled={busy !== undefined || status?.connection.state === "disconnected"}
        >
          {busy === "disconnect" ? null : <PowerIcon size={14} />}
          {t.disconnect}
        </Button>
      </div>
    </header>
  );
}
