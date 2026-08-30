import { zhCN as t } from "../i18n";
import type { StatusSnapshot } from "../types";
import { Badge } from "../ui/Badge";
import { Button } from "../ui/Button";
import { Card, Fact } from "../ui/Card";
import { Notice, Spinner } from "../ui/Feedback";
import { GlobeIcon, PowerIcon } from "../ui/Icon";
import { riskTone } from "../ui/Meter";
import { formatBits, formatElapsed } from "../utils/format";
import { connectionLabel, connectionTone } from "../utils/status";

export function ActiveExitCard({ status, busy, now, onDisconnect, onBrowseNodes }: {
  status: StatusSnapshot | undefined;
  busy: string | undefined;
  now: number;
  onDisconnect: () => void;
  onBrowseNodes: () => void;
}) {
  const connection = status?.connection;
  const node = status?.activeNode;
  const tone = connectionTone(status);

  if (connection === undefined || connection.state === "disconnected" || node === undefined) {
    return (
      <Card title={t.activeExit} description={connectionLabel(connection)}>
        <div className="exit__empty">
          <GlobeIcon size={26} />
          <strong>{t.activeExitIdle}</strong>
          <p>{t.activeExitIdleHint}</p>
          <Button variant="primary" onClick={onBrowseNodes}>{t.navNodes}</Button>
        </div>
      </Card>
    );
  }

  const result = node.latestTest?.result;
  const since = connection.state === "connected" || connection.state === "connecting"
    ? connection.since
    : undefined;

  return (
    <Card
      title={t.activeExit}
      actions={
        <Badge tone={tone === "online" ? "accent" : tone === "danger" ? "danger" : "warning"}>
          {connection.state === "connecting" && <Spinner />}
          {connectionLabel(connection)}
        </Badge>
      }
      footer={
        <>
          <span className="muted">
            {status?.proxyReady === true ? `${t.proxy} · ${t.proxyReady}` : `${t.proxy} · ${t.proxyClosed}`}
          </span>
          <Button
            variant="danger"
            busy={busy === "disconnect"}
            disabled={busy !== undefined}
            onClick={onDisconnect}
          >
            <PowerIcon size={14} />
            {t.disconnect}
          </Button>
        </>
      }
    >
      <div className="exit">
        <div className="exit__identity">
          <span className="country-chip">{node.countryShort}</span>
          <span className="exit__names">
            <strong title={node.hostname}>{node.hostname}</strong>
            <span>{node.countryLong} · {node.operator || t.operatorUnknown}</span>
          </span>
        </div>

        {connection.state === "failed" && <Notice tone="danger">{connection.message}</Notice>}

        <div className="exit__facts">
          <Fact label={t.latency}>{node.pingMs === null ? "—" : `${node.pingMs} ms`}</Fact>
          <Fact label={t.bandwidth}>{formatBits(node.speedBps)}</Fact>
          <Fact label={t.exitIp}>
            <span className="mono">{result?.exitIp ?? node.ip}</span>
          </Fact>
          <Fact label={t.fraudScore}>
            {result === undefined ? (
              <span className="muted">{t.notTested}</span>
            ) : (
              <span className={`risk-value--${riskTone(result.fraudScore)}`}>
                {Math.round(result.fraudScore)}
              </span>
            )}
          </Fact>
          <Fact label={t.connectedSince}>
            {since === undefined ? "—" : formatElapsed(since, now)}
          </Fact>
          <Fact label={t.worker}>
            <span className="mono">
              {connection.state === "failed" ? "—" : connection.workerId.slice(0, 8)}
            </span>
          </Fact>
        </div>
      </div>
    </Card>
  );
}
