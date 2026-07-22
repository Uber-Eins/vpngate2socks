import { FormEvent, useCallback, useEffect, useState } from "react";

import * as api from "./api";
import { zhCN as t } from "./i18n";
import type { NodesPage, SessionState, StatusSnapshot, TestRecord, TestState, VpnNode } from "./types";

const EMPTY_PAGE: NodesPage = { items: [], page: 1, pageSize: 50, total: 0 };

export function App() {
  const [session, setSession] = useState<SessionState>();
  const [status, setStatus] = useState<StatusSnapshot>();
  const [page, setPage] = useState(EMPTY_PAGE);
  const [pageNumber, setPageNumber] = useState(1);
  const [draftSearch, setDraftSearch] = useState("");
  const [search, setSearch] = useState("");
  const [sort, setSort] = useState("score");
  const [order, setOrder] = useState("desc");
  const [operations, setOperations] = useState<Record<string, { id: string; state: TestState }>>({});
  const [busy, setBusy] = useState<string>();
  const [error, setError] = useState<string>();

  const load = useCallback(async () => {
    const [nextStatus, nextPage] = await Promise.all([
      api.status(),
      api.nodes({ page: pageNumber, pageSize: 50, search, sort, order })
    ]);
    setStatus(nextStatus);
    setPage(nextPage);
  }, [order, pageNumber, search, sort]);

  useEffect(() => {
    void api.session().then(setSession).catch((reason: unknown) => setError(message(reason)));
  }, []);

  useEffect(() => {
    if (session?.authenticated !== true) return;
    void load().catch((reason: unknown) => setError(message(reason)));
    const events = new EventSource("/api/v1/events");
    const reload = () => void load().catch((reason: unknown) => setError(message(reason)));
    events.addEventListener("update", reload);
    events.addEventListener("resync", reload);
    return () => events.close();
  }, [load, session]);

  useEffect(() => {
    const pending = Object.values(operations).filter(({ state }) =>
      state.state === "queued" || state.state === "running"
    );
    if (pending.length === 0) return;
    const timer = window.setInterval(() => {
      for (const operation of pending) {
        void api.testStatus(operation.id).then((state) => {
          setOperations((current) => ({
            ...current,
            [state.nodeId]: { id: operation.id, state }
          }));
          if (state.state === "succeeded" || state.state === "failed") {
            void load();
          }
        }).catch((reason: unknown) => setError(message(reason)));
      }
    }, 1000);
    return () => window.clearInterval(timer);
  }, [load, operations]);

  const act = async (key: string, action: () => Promise<unknown>) => {
    setBusy(key);
    setError(undefined);
    try {
      await action();
      await load();
    } catch (reason) {
      setError(message(reason));
    } finally {
      setBusy(undefined);
    }
  };

  const logout = async () => {
    setBusy("logout");
    setError(undefined);
    try {
      await api.logout();
      setSession({ authenticated: false });
      setStatus(undefined);
      setPage(EMPTY_PAGE);
    } catch (reason) {
      setError(message(reason));
    } finally {
      setBusy(undefined);
    }
  };

  if (session === undefined) return <Loading />;
  if (!session.authenticated) return <Login onAuthenticated={setSession} />;

  const totalPages = Math.max(1, Math.ceil(page.total / page.pageSize));
  return (
    <main className="shell">
      <header className="hero">
        <div>
          <p className="eyebrow">{t.eyebrow}</p>
          <h1>{t.title}</h1>
          <p className="subtitle">{t.subtitle}</p>
        </div>
        <StatusCard status={status} />
      </header>

      {status?.lanMode === true && !status.tlsConfigured && (
        <Notice tone="danger">{t.cleartextWarning}</Notice>
      )}
      <Notice tone="neutral">{t.browserWarning}</Notice>
      {error !== undefined && <Notice tone="danger">{error}</Notice>}

      <section className="toolbar" aria-label="节点工具栏">
        <form
          className="search"
          onSubmit={(event) => {
            event.preventDefault();
            setPageNumber(1);
            setSearch(draftSearch.trim());
          }}
        >
          <input
            aria-label={t.searchPlaceholder}
            value={draftSearch}
            onChange={(event) => setDraftSearch(event.target.value)}
            placeholder={t.searchPlaceholder}
          />
          <button type="submit">{t.search}</button>
        </form>
        <div className="toolbar-actions">
          <select aria-label="排序字段" value={sort} onChange={(event) => setSort(event.target.value)}>
            <option value="score">评分</option>
            <option value="ping">Ping</option>
            <option value="speed">带宽</option>
            <option value="sessions">会话数</option>
          </select>
          <select aria-label="排序方向" value={order} onChange={(event) => setOrder(event.target.value)}>
            <option value="desc">降序</option>
            <option value="asc">升序</option>
          </select>
          <button disabled={busy !== undefined} onClick={() => void act("refresh", api.refresh)}>
            {busy === "refresh" ? "刷新中…" : t.refresh}
          </button>
          <button
            className="ghost danger"
            disabled={status?.proxyReady !== true || busy !== undefined}
            onClick={() => void act("disconnect", api.disconnect)}
          >
            {t.disconnect}
          </button>
          {session.csrfToken !== undefined && (
            <button className="ghost" disabled={busy !== undefined} onClick={() => void logout()}>
              {t.logout}
            </button>
          )}
        </div>
      </section>

      <NodeTable
        nodes={page.items}
        activeNodeId={activeNodeId(status)}
        operations={operations}
        busy={busy}
        onConnect={(nodeId) => void act(`connect:${nodeId}`, () => api.connect(nodeId))}
        onTest={(nodeId) => {
          void act(`test:${nodeId}`, async () => {
            const id = await api.startTest(nodeId);
            setOperations((current) => ({
              ...current,
              [nodeId]: {
                id,
                state: { state: "queued", nodeId, queuedAt: new Date().toISOString() }
              }
            }));
          });
        }}
      />

      <footer className="pager">
        <span>{page.total} 个节点 · 第 {page.page}/{totalPages} 页</span>
        <div>
          <button disabled={pageNumber <= 1} onClick={() => setPageNumber((value) => value - 1)}>
            {t.previous}
          </button>
          <button disabled={pageNumber >= totalPages} onClick={() => setPageNumber((value) => value + 1)}>
            {t.next}
          </button>
        </div>
      </footer>
    </main>
  );
}

export function NodeTable(props: {
  nodes: VpnNode[];
  activeNodeId?: string | undefined;
  operations: Record<string, { id: string; state: TestState }>;
  busy?: string | undefined;
  onConnect: (nodeId: string) => void;
  onTest: (nodeId: string) => void;
}) {
  if (props.nodes.length === 0) return <div className="empty">{t.empty}</div>;
  return (
    <div className="table-wrap">
      <table>
        <thead>
          <tr>
            <th>{t.node}</th><th>{t.region}</th><th>{t.quality}</th><th>{t.sessions}</th>
            <th>{t.risk}</th><th>{t.actions}</th>
          </tr>
        </thead>
        <tbody>
          {props.nodes.map((node) => {
            const operation = props.operations[node.id]?.state;
            const active = props.activeNodeId === node.id;
            return (
              <tr key={node.id} className={active ? "active-row" : undefined}>
                <td>
                  <div className="node-name"><span className="flag">{node.countryShort}</span>{node.hostname}</div>
                  <div className="mono muted">{node.ip}:{node.tcpPort ?? "—"}</div>
                </td>
                <td><strong>{node.countryLong}</strong><div className="muted">{node.operator || "—"}</div></td>
                <td>
                  <strong>{node.pingMs === null ? "—" : `${node.pingMs} ms`}</strong>
                  <div className="muted">{formatBits(node.speedBps)} · 分数 {node.score}</div>
                </td>
                <td><strong>{node.sessions}</strong><div className="muted">累计 {node.totalUsers}</div></td>
                <td><Risk record={node.latestTest} operation={operation} /></td>
                <td>
                  <div className="row-actions">
                    <button
                      className={active ? "connected" : undefined}
                      disabled={node.availability !== "available" || props.busy !== undefined || active}
                      onClick={() => props.onConnect(node.id)}
                    >
                      {active ? t.connected : node.availability === "available" ? t.connect : t.unavailable}
                    </button>
                    <button
                      className="ghost"
                      disabled={node.availability !== "available" || props.busy !== undefined || operation?.state === "queued" || operation?.state === "running"}
                      onClick={() => props.onTest(node.id)}
                    >
                      {operation?.state === "queued" ? t.queued : operation?.state === "running" ? t.running : t.test}
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

function Risk({ record, operation }: {
  record?: VpnNode["latestTest"] | undefined;
  operation?: TestState | undefined;
}) {
  const effective = operation?.state === "succeeded" || operation?.state === "failed"
    ? operation.record
    : record;
  if (operation?.state === "queued" || operation?.state === "running") {
    return <span className="pill pending">{operation.state === "queued" ? t.queued : t.running}</span>;
  }
  if (effective?.error !== undefined) {
    return (
      <div>
        <span className="error-detail" title={effective.error}>{effective.error}</span>
        <TestMeta record={effective} />
      </div>
    );
  }
  if (effective?.result === undefined) return <span className="muted">{t.noResult}</span>;
  return (
    <div className="risk-grid">
      <span className="score">{t.fraudScore} <strong>{effective.result.fraudScore}</strong></span>
      <span className={`pill ${effective.result.isResidential ? "good" : "warn"}`}>
        {effective.result.isResidential ? t.residential : t.nonResidential}
      </span>
      <span className={`pill ${effective.result.isBroadcast ? "warn" : "good"}`}>
        {effective.result.isBroadcast ? t.broadcast : t.notBroadcast}
      </span>
      {effective.result.exitIp !== undefined && <span className="mono muted">{effective.result.exitIp}</span>}
      <TestMeta record={effective} />
    </div>
  );
}

function TestMeta({ record }: { record: TestRecord }) {
  return (
    <span className="test-meta muted" title={new Date(record.testedAt).toLocaleString("zh-CN")}>
      {record.durationMs} ms · {new Date(record.testedAt).toLocaleString("zh-CN")}
    </span>
  );
}

function StatusCard({ status }: { status?: StatusSnapshot | undefined }) {
  const connection = status?.connection;
  const label = connection?.state === "connected" ? t.connected
    : connection?.state === "connecting" ? t.connecting
    : connection?.state === "failed" ? t.failed : t.disconnected;
  return (
    <aside className="status-card">
      <div className="status-line"><span className={`pulse ${status?.proxyReady === true ? "on" : ""}`} />{label}</div>
      <strong>{status?.proxyReady === true ? t.ready : t.notReady}</strong>
      <div className="status-meta">
        <span>{t.accepted}: {status?.lastRefresh?.accepted ?? "—"}</span>
        <span>{t.tests}: {(status?.queuedTests ?? 0) + (status?.runningTests ?? 0)}</span>
      </div>
      {upstreamWarning(status) !== undefined && <span className="danger-text">{upstreamWarning(status)}</span>}
      {connection?.state === "failed" && <span className="danger-text">{connection.message}</span>}
    </aside>
  );
}

function Login({ onAuthenticated }: { onAuthenticated: (session: SessionState) => void }) {
  const [username, setUsername] = useState("");
  const [password, setPassword] = useState("");
  const [error, setError] = useState<string>();
  const submit = async (event: FormEvent) => {
    event.preventDefault();
    try {
      onAuthenticated(await api.login(username, password));
      setPassword("");
    } catch (reason) {
      setError(message(reason));
      setPassword("");
    }
  };
  return (
    <main className="login-shell">
      <form className="login-card" onSubmit={(event) => void submit(event)}>
        <p className="eyebrow">{t.brand}</p><h1>{t.loginTitle}</h1>
        {window.location.protocol !== "https:" && <Notice tone="danger">{t.cleartextWarning}</Notice>}
        <label>{t.username}<input autoComplete="username" value={username} onChange={(event) => setUsername(event.target.value)} /></label>
        <label>{t.password}<input type="password" autoComplete="current-password" value={password} onChange={(event) => setPassword(event.target.value)} /></label>
        {error !== undefined && <p className="danger-text">{error}</p>}
        <button type="submit">{t.login}</button>
      </form>
    </main>
  );
}

function Notice({ children, tone }: { children: string; tone: "neutral" | "danger" }) {
  return <div className={`notice ${tone}`}>{children}</div>;
}

function Loading() {
  return <main className="loading"><span className="pulse on" />正在启动安全控制面…</main>;
}

function activeNodeId(status?: StatusSnapshot | undefined): string | undefined {
  const connection = status?.connection;
  return connection?.state === "connected" ? connection.nodeId : undefined;
}

function upstreamWarning(status?: StatusSnapshot | undefined): string | undefined {
  switch (status?.upstreamState) {
    case "checking": return t.upstreamChecking;
    case "unreachable": return t.upstreamUnreachable;
    case "authenticationFailed": return t.upstreamAuthenticationFailed;
    case "netdUnavailable": return t.upstreamDown;
    default: return undefined;
  }
}

function formatBits(bytesPerSecond: number): string {
  const bits = bytesPerSecond * 8;
  if (bits >= 1_000_000_000) return `${(bits / 1_000_000_000).toFixed(1)} Gbps`;
  if (bits >= 1_000_000) return `${(bits / 1_000_000).toFixed(1)} Mbps`;
  return `${Math.round(bits / 1000)} Kbps`;
}

function message(reason: unknown): string {
  return reason instanceof Error ? reason.message : "请求失败";
}
