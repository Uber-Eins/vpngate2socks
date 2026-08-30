import { zhCN as t } from "../i18n";
import type { VpnNode } from "../types";
import { Badge } from "../ui/Badge";
import { Button } from "../ui/Button";
import { Fact } from "../ui/Card";
import { Dialog } from "../ui/Dialog";
import { RiskMeter, riskTone } from "../ui/Meter";
import { formatBits, formatBytes, formatDateTime, formatDuration, formatNumber } from "../utils/format";
import { availabilityLabel } from "../utils/status";

export function NodeDetailDialog({ node, busy, activeNodeId, onConnect, onTest, onClose }: {
  node: VpnNode | undefined;
  busy: string | undefined;
  activeNodeId: string | undefined;
  onConnect: (nodeId: string) => void;
  onTest: (nodeId: string) => void;
  onClose: () => void;
}) {
  const eligible = node?.availability === "available";
  const active = node !== undefined && node.id === activeNodeId;
  const test = node?.latestTest;
  const result = test?.result;

  return (
    <Dialog
      open={node !== undefined}
      title={node?.hostname ?? t.nodeDetail}
      {...(node === undefined ? {} : { subtitle: `${node.ip}:${node.tcpPort ?? "—"}` })}
      closeLabel={t.close}
      onClose={onClose}
      footer={
        node === undefined ? null : (
          <>
            <Button
              disabled={!eligible || busy !== undefined}
              busy={busy === `test:${node.id}`}
              onClick={() => onTest(node.id)}
            >
              {test === undefined ? t.prioritizeTest : t.retest}
            </Button>
            <Button
              variant="primary"
              disabled={!eligible || busy !== undefined || active}
              busy={busy === `connect:${node.id}`}
              onClick={() => onConnect(node.id)}
            >
              {active ? t.connected : t.connect}
            </Button>
          </>
        )
      }
    >
      {node !== undefined && (
        <>
          <section className="detail-section">
            <h3>{t.sectionIdentity}</h3>
            <div className="detail-grid">
              <Fact label={t.region}>{node.countryLong} · {node.countryShort}</Fact>
              <Fact label={t.address}><span className="mono">{node.ip}</span></Fact>
              <Fact label={t.tcpPort}>{node.tcpPort ?? "—"}</Fact>
              <Fact label={t.availability}>
                <Badge tone={eligible ? "accent" : "warning"}>
                  {availabilityLabel(node.availability)}
                </Badge>
              </Fact>
            </div>
          </section>

          <section className="detail-section">
            <h3>{t.sectionPerformance}</h3>
            <div className="detail-grid">
              <Fact label={t.latency}>{node.pingMs === null ? "—" : `${node.pingMs} ms`}</Fact>
              <Fact label={t.bandwidth}>{formatBits(node.speedBps)}</Fact>
              <Fact label={t.score}>{formatNumber(node.score)}</Fact>
              <Fact label={t.sessions}>{formatNumber(node.sessions)}</Fact>
              <Fact label={t.uptime}>{formatDuration(node.uptimeMs)}</Fact>
              <Fact label={t.totalUsers}>{formatNumber(node.totalUsers)}</Fact>
              <Fact label={t.totalTraffic}>{formatBytes(node.totalTrafficBytes)}</Fact>
              <Fact label={t.logType}>{node.logType || "—"}</Fact>
            </div>
          </section>

          <section className="detail-section">
            <h3>{t.sectionIpPure}</h3>
            {result !== undefined ? (
              <>
                <div className="detail-grid">
                  <Fact label={t.fraudScore}>
                    <span className={`risk-value--${riskTone(result.fraudScore)}`}>
                      {result.fraudScore}
                    </span>
                  </Fact>
                  <Fact label={t.exitIp}>
                    <span className="mono">{result.exitIp ?? "—"}</span>
                  </Fact>
                  <Fact label={t.testedAt}>{formatDateTime(test?.testedAt ?? "")}</Fact>
                  <Fact label={t.testDuration}>{test?.durationMs ?? 0} ms</Fact>
                </div>
                <RiskMeter score={result.fraudScore} label={`${t.fraudScore} ${result.fraudScore}`} />
                <div className="risk-cell__tags">
                  <Badge tone={result.isResidential ? "accent" : "neutral"}>
                    {result.isResidential ? t.residential : t.nonResidential}
                  </Badge>
                  <Badge tone={result.isBroadcast ? "warning" : "neutral"}>
                    {result.isBroadcast ? t.broadcast : t.notBroadcast}
                  </Badge>
                </div>
              </>
            ) : (
              <p className="detail-message">
                {test?.error === undefined ? t.notTested : `${t.testFailed} · ${test.error}`}
              </p>
            )}
          </section>

          <section className="detail-section">
            <h3>{t.sectionOperator}</h3>
            <div className="detail-grid">
              <Fact label={t.operator}>{node.operator || t.operatorUnknown}</Fact>
            </div>
            {node.message !== "" && <p className="detail-message">{node.message}</p>}
            <p className="detail-message mono">{node.id}</p>
          </section>
        </>
      )}
    </Dialog>
  );
}
