import { zhCN as t } from "../i18n";
import { ActiveExitCard } from "../overview/ActiveExitCard";
import { PolicyRecapCard } from "../overview/PolicyRecapCard";
import type { ConsoleModel } from "../state/useConsole";
import { useNow } from "../state/useNow";
import type { View } from "../state/useView";
import { Card, Fact } from "../ui/Card";
import { Notice } from "../ui/Feedback";
import { ActivityIcon, FlaskIcon, ServerIcon, ShieldIcon } from "../ui/Icon";
import { StatTile } from "../ui/StatTile";
import { formatDateTime, formatNumber, formatRelative } from "../utils/format";
import { connectionLabel, upstreamLabel } from "../utils/status";

export function OverviewView({ model, onNavigate }: {
  model: ConsoleModel;
  onNavigate: (view: View) => void;
}) {
  const now = useNow();
  const status = model.status;
  const refresh = status?.lastRefresh;
  const upstreamDegraded = status !== undefined && status.upstreamState !== "ready";
  const cleartextLan = status?.lanMode === true && !status.tlsConfigured;

  return (
    <>
      <div className="view-heading">
        <div>
          <h1>{t.overviewTitle}</h1>
          <p>{t.overviewSubtitle}</p>
        </div>
      </div>

      {cleartextLan && (
        <div className="notice-stack">
          <Notice tone="danger">{t.tlsOffLan}</Notice>
        </div>
      )}

      <div className="tile-row">
        <StatTile
          icon={<ShieldIcon size={14} />}
          label={t.proxy}
          value={status?.proxyReady === true ? t.proxyReady : t.proxyClosed}
          meta={connectionLabel(status?.connection)}
          tone={status?.proxyReady === true ? "accent" : "default"}
        />
        <StatTile
          icon={<ActivityIcon size={14} />}
          label={t.upstream}
          value={upstreamLabel(status?.upstreamState)}
          tone={upstreamDegraded ? "danger" : "default"}
        />
        <StatTile
          icon={<ServerIcon size={14} />}
          label={t.availableNodes}
          value={refresh === undefined ? "—" : formatNumber(refresh.accepted)}
          meta={refresh === undefined ? t.never : `${t.lastRefreshAt} ${formatRelative(refresh.at, now)}`}
        />
        <StatTile
          icon={<FlaskIcon size={14} />}
          label={t.testQueue}
          value={status === undefined ? "—" : `${status.runningTests} / ${status.queuedTests}`}
          unit={`${t.running} / ${t.queued}`}
        />
      </div>

      <div className="grid-2">
        <ActiveExitCard
          status={status}
          busy={model.busy}
          now={now}
          onDisconnect={model.disconnect}
          onBrowseNodes={() => onNavigate("nodes")}
        />
        <PolicyRecapCard settings={model.autoConnect} onEdit={() => onNavigate("policy")} />
      </div>

      <Card title={t.snapshot} description={t.nodesSubtitle}>
        <div className="exit__facts">
          <Fact label={t.accepted}>{refresh === undefined ? "—" : formatNumber(refresh.accepted)}</Fact>
          <Fact label={t.rejected}>{refresh === undefined ? "—" : formatNumber(refresh.rejected)}</Fact>
          <Fact label={t.unsupported}>
            {refresh === undefined ? "—" : formatNumber(refresh.unsupported)}
          </Fact>
          <Fact label={t.lastRefreshAt}>
            {refresh === undefined ? t.never : formatDateTime(refresh.at)}
          </Fact>
        </div>
      </Card>
    </>
  );
}
