import { zhCN as t } from "../i18n";
import { NodeTable } from "../nodes/NodeTable";
import { NodeToolbar } from "../nodes/NodeToolbar";
import { Pagination } from "../nodes/Pagination";
import type { SessionState } from "../types";
import { Notice } from "../ui/Notice";
import { DashboardHeader } from "./DashboardHeader";
import { useDashboard } from "./useDashboard";

export function Dashboard({ session, onLoggedOut }: {
  session: SessionState;
  onLoggedOut: () => void;
}) {
  const model = useDashboard(onLoggedOut);
  const totalPages = Math.max(1, Math.ceil(model.page.total / model.page.pageSize));
  const connection = model.status?.connection;
  const activeNodeId = connection?.state === "connected" ? connection.nodeId : undefined;

  return (
    <main className="app-shell">
      <DashboardHeader status={model.status} />

      <div className="notice-stack">
        {model.status?.lanMode === true && !model.status.tlsConfigured && (
          <Notice tone="danger">{t.cleartextWarning}</Notice>
        )}
        <Notice tone="neutral">{t.browserWarning}</Notice>
        {model.error !== undefined && <Notice tone="danger">{model.error}</Notice>}
      </div>

      <section className="node-directory" aria-labelledby="node-directory-title">
        <div className="section-heading">
          <div>
            <p className="section-kicker">ROUTE INVENTORY</p>
            <h2 id="node-directory-title">{t.nodeDirectory}</h2>
          </div>
          <p>{t.nodeDirectoryHint}</p>
        </div>
        <NodeToolbar
          draftSearch={model.draftSearch}
          sort={model.sort}
          order={model.order}
          busy={model.busy}
          canDisconnect={model.status?.proxyReady === true}
          canLogout={session.csrfToken !== undefined}
          onDraftSearch={model.setDraftSearch}
          onSearch={model.applySearch}
          onSort={model.changeSort}
          onOrder={model.changeOrder}
          onRefresh={model.refresh}
          onDisconnect={model.disconnect}
          onLogout={model.logout}
        />
        <NodeTable
          nodes={model.page.items}
          activeNodeId={activeNodeId}
          operations={model.operations}
          busy={model.busy}
          onConnect={model.connect}
          onTest={model.test}
        />
        <Pagination
          page={model.page.page}
          totalPages={totalPages}
          totalItems={model.page.total}
          onPage={model.setPageNumber}
        />
      </section>
    </main>
  );
}
