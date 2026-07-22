import { zhCN as t } from "../i18n";
import type { TestOperations, VpnNode } from "../types";
import { formatBits } from "../utils/formatBits";
import { RiskSummary } from "./RiskSummary";

export function NodeTable(props: {
  nodes: VpnNode[];
  activeNodeId: string | undefined;
  operations: TestOperations;
  busy: string | undefined;
  onConnect: (nodeId: string) => void;
  onTest: (nodeId: string) => void;
}) {
  if (props.nodes.length === 0) {
    return (
      <div className="empty-state">
        <span aria-hidden="true" />
        <strong>{t.empty}</strong>
        <p>尝试更换关键词，或刷新 VPN Gate 节点快照。</p>
      </div>
    );
  }

  return (
    <div className="node-table-wrap">
      <table className="node-table">
        <thead>
          <tr>
            <th>{t.node}</th>
            <th>{t.region}</th>
            <th>{t.quality}</th>
            <th className="optional-column">{t.sessions}</th>
            <th>{t.risk}</th>
            <th>{t.actions}</th>
          </tr>
        </thead>
        <tbody>
          {props.nodes.map((node) => {
            const operation = props.operations[node.id]?.state;
            const active = props.activeNodeId === node.id;
            const eligible = node.availability === "available";
            const testing = operation?.state === "queued" || operation?.state === "running";
            const testLabel = operation?.state === "queued"
              ? t.queued
              : operation?.state === "running"
                ? t.running
                : node.latestTest === undefined
                  ? t.prioritizeTest
                  : t.retest;
            return (
              <tr key={node.id} className={active ? "node-row node-row--active" : "node-row"}>
                <td>
                  <div className="node-identity">
                    <span className="country-code">{node.countryShort}</span>
                    <div>
                      <strong>{node.hostname}</strong>
                      <code>{node.ip}:{node.tcpPort ?? "—"}</code>
                    </div>
                    {active && <span className="active-badge">ACTIVE</span>}
                  </div>
                </td>
                <td>
                  <div className="region-cell">
                    <strong>{node.countryLong}</strong>
                    <span title={node.operator}>{node.operator || "未知运营者"}</span>
                  </div>
                </td>
                <td>
                  <div className="quality-cell">
                    <strong>{node.pingMs === null ? "—" : `${node.pingMs} ms`}</strong>
                    <span>{formatBits(node.speedBps)}</span>
                    <small>Score {node.score.toLocaleString()}</small>
                  </div>
                </td>
                <td className="optional-column">
                  <div className="sessions-cell">
                    <strong>{node.sessions}</strong>
                    <span>累计 {node.totalUsers.toLocaleString()}</span>
                  </div>
                </td>
                <td className="risk-cell">
                  <RiskSummary record={node.latestTest} operation={operation} eligible={eligible} />
                </td>
                <td>
                  <div className="row-actions">
                    <button
                      className={active ? "button button--connected" : "button button--primary"}
                      type="button"
                      disabled={!eligible || props.busy !== undefined || active}
                      onClick={() => props.onConnect(node.id)}
                    >
                      {active
                        ? t.connected
                        : !eligible
                          ? t.unavailable
                          : props.busy === `connect:${node.id}`
                            ? t.connecting
                            : t.connect}
                    </button>
                    <button
                      className="button button--quiet"
                      type="button"
                      disabled={!eligible || props.busy !== undefined || testing}
                      onClick={() => props.onTest(node.id)}
                    >
                      {props.busy === `test:${node.id}` ? "提交中…" : testLabel}
                    </button>
                  </div>
                </td>
              </tr>
            );
          })}
        </tbody>
      </table>
    </div>
  );
}
