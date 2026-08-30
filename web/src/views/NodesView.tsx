import { useState } from "react";

import { zhCN as t } from "../i18n";
import { NodeDetailDialog } from "../nodes/NodeDetailDialog";
import { NodeFilters } from "../nodes/NodeFilters";
import { NodeTable } from "../nodes/NodeTable";
import { Pagination } from "../nodes/Pagination";
import type { ConsoleModel } from "../state/useConsole";
import type { VpnNode } from "../types";

export function NodesView({ model }: { model: ConsoleModel }) {
  const [inspected, setInspected] = useState<VpnNode>();
  const totalPages = Math.max(1, Math.ceil(model.page.total / model.page.pageSize));
  const connection = model.status?.connection;
  const activeNodeId = connection?.state === "connected" ? connection.nodeId : undefined;

  return (
    <>
      <div className="view-heading">
        <div>
          <h1>{t.nodesTitle}</h1>
          <p>{t.nodesSubtitle}</p>
        </div>
      </div>

      <section className="card card--flush">
        <NodeFilters
          query={model.query}
          draftSearch={model.draftSearch}
          regions={model.autoConnect?.regions ?? []}
          busy={model.busy !== undefined}
          onDraftSearch={model.setDraftSearch}
          onSearch={model.applySearch}
          onFilter={model.setFilter}
          onReset={model.resetFilters}
        />
        <NodeTable
          nodes={model.page.items}
          activeNodeId={activeNodeId}
          operations={model.operations}
          busy={model.busy}
          loading={model.loadingNodes}
          sort={model.query.sort}
          order={model.query.order}
          onSort={model.toggleSort}
          onConnect={model.connect}
          onTest={model.test}
          onInspect={setInspected}
        />
        <Pagination
          page={model.page.page}
          totalPages={totalPages}
          totalItems={model.page.total}
          onPage={model.setPageNumber}
        />
      </section>

      <NodeDetailDialog
        node={inspected}
        busy={model.busy}
        activeNodeId={activeNodeId}
        onConnect={(nodeId) => {
          model.connect(nodeId);
          setInspected(undefined);
        }}
        onTest={model.test}
        onClose={() => setInspected(undefined)}
      />
    </>
  );
}
