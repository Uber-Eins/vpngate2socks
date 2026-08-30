import { zhCN as t } from "../i18n";
import type { SortKey, SortOrder, TestOperations, VpnNode } from "../types";
import { Button } from "../ui/Button";
import { EmptyState, Skeleton } from "../ui/Feedback";
import { ArrowDownIcon, ArrowUpIcon, SearchIcon } from "../ui/Icon";
import { cx } from "../utils/cx";
import { formatBits } from "../utils/format";
import { RiskCell } from "./RiskCell";

export interface NodeTableProps {
  nodes: VpnNode[];
  activeNodeId: string | undefined;
  operations: TestOperations;
  busy: string | undefined;
  loading: boolean;
  sort: SortKey;
  order: SortOrder;
  onSort: (key: SortKey) => void;
  onConnect: (nodeId: string) => void;
  onTest: (nodeId: string) => void;
  onInspect: (node: VpnNode) => void;
}

export function NodeTable(props: NodeTableProps) {
  if (props.loading && props.nodes.length === 0) {
    return (
      <div className="empty">
        <div className="skeleton-stack" aria-label={t.loadingNodes}>
          <Skeleton />
          <Skeleton />
          <Skeleton width="half" />
        </div>
      </div>
    );
  }

  if (props.nodes.length === 0) {
    return (
      <EmptyState
        icon={<SearchIcon size={26} />}
        title={t.emptyTitle}
        description={t.emptyHint}
      />
    );
  }

  const header = (key: SortKey, label: string, className?: string) => (
    <SortHeader
      label={label}
      className={className}
      active={props.sort === key}
      order={props.order}
      onSort={() => props.onSort(key)}
    />
  );

  return (
    <div className="table-scroll">
      <table className="table">
        <thead>
          <tr>
            <th scope="col">{t.columnNode}</th>
            <th scope="col" className="optional-md">{t.columnRegion}</th>
            {header("score", t.score, "col-num optional-sm")}
            {header("ping", t.columnPing, "col-num")}
            {header("speed", t.columnSpeed, "col-num")}
            {header("sessions", t.columnSessions, "col-num optional-sm")}
            {header("fraud", t.columnRisk)}
            <th scope="col" className="col-num">{t.columnActions}</th>
          </tr>
        </thead>
        <tbody>
          {props.nodes.map((node) => (
            <NodeRow key={node.id} node={node} {...props} />
          ))}
        </tbody>
      </table>
    </div>
  );
}

function NodeRow({ node, activeNodeId, operations, busy, onConnect, onTest, onInspect }:
  NodeTableProps & { node: VpnNode }) {
  const operation = operations[node.id]?.state;
  const active = activeNodeId === node.id;
  const eligible = node.availability === "available";
  const testing = operation?.state === "queued" || operation?.state === "running";
  const testLabel = node.latestTest === undefined ? t.prioritizeTest : t.retest;

  return (
    <tr className={cx(active && "row--active")}>
      <td>
        <div className="node-cell">
          <span className="country-chip">{node.countryShort}</span>
          <span className="node-cell__text">
            <button
              type="button"
              className="node-cell__name"
              title={`${node.hostname} · ${t.details}`}
              onClick={() => onInspect(node)}
            >
              {node.hostname}
            </button>
            <span className="node-cell__address">{node.ip}:{node.tcpPort ?? "—"}</span>
          </span>
        </div>
      </td>
      <td className="optional-md">
        <span className="region-cell">
          <strong>{node.countryLong}</strong>
          <span title={node.operator}>{node.operator || t.operatorUnknown}</span>
        </span>
      </td>
      <td className="col-num optional-sm">{node.score.toLocaleString("zh-CN")}</td>
      <td className="col-num">{node.pingMs === null ? "—" : `${node.pingMs} ms`}</td>
      <td className="col-num">{formatBits(node.speedBps)}</td>
      <td className="col-num optional-sm">{node.sessions}</td>
      <td>
        <RiskCell record={node.latestTest} operation={operation} eligible={eligible} />
      </td>
      <td>
        <div className="row-actions">
          <Button
            variant={active || !eligible ? "default" : "primary"}
            size="sm"
            disabled={!eligible || busy !== undefined || active}
            busy={busy === `connect:${node.id}`}
            onClick={() => onConnect(node.id)}
          >
            {active ? t.connected : eligible ? t.connect : t.unavailableInV1}
          </Button>
          <Button
            size="sm"
            disabled={!eligible || busy !== undefined || testing}
            busy={busy === `test:${node.id}`}
            onClick={() => onTest(node.id)}
          >
            {testLabel}
          </Button>
        </div>
      </td>
    </tr>
  );
}

function SortHeader({ label, className, active, order, onSort }: {
  label: string;
  className?: string | undefined;
  active: boolean;
  order: SortOrder;
  onSort: () => void;
}) {
  return (
    <th
      scope="col"
      className={className}
      aria-sort={active ? (order === "asc" ? "ascending" : "descending") : "none"}
    >
      <button
        type="button"
        className={cx("sort-button", active && "sort-button--active")}
        onClick={onSort}
      >
        {label}
        <span className="sort-button__arrow">
          {order === "asc" ? <ArrowUpIcon size={12} /> : <ArrowDownIcon size={12} />}
        </span>
      </button>
    </th>
  );
}
