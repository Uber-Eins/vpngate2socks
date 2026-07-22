import { useCallback, useEffect, useRef, useState } from "react";

import * as api from "../api";
import { parseAppEvent } from "../events/parseAppEvent";
import type {
  NodesPage,
  StatusSnapshot,
  TestOperations,
  TestState
} from "../types";
import { errorMessage } from "../utils/errorMessage";

const PAGE_SIZE = 50;
const EMPTY_PAGE: NodesPage = { items: [], page: 1, pageSize: PAGE_SIZE, total: 0 };

export interface DashboardModel {
  status: StatusSnapshot | undefined;
  page: NodesPage;
  draftSearch: string;
  sort: string;
  order: string;
  operations: TestOperations;
  busy: string | undefined;
  error: string | undefined;
  setDraftSearch: (value: string) => void;
  applySearch: () => void;
  changeSort: (value: string) => void;
  changeOrder: (value: string) => void;
  setPageNumber: (value: number) => void;
  refresh: () => void;
  disconnect: () => void;
  connect: (nodeId: string) => void;
  test: (nodeId: string) => void;
  logout: () => void;
}

export function useDashboard(onLoggedOut: () => void): DashboardModel {
  const [status, setStatus] = useState<StatusSnapshot>();
  const [page, setPage] = useState(EMPTY_PAGE);
  const [pageNumber, setPageNumberState] = useState(1);
  const [draftSearch, setDraftSearch] = useState("");
  const [search, setSearch] = useState("");
  const [sort, setSort] = useState("score");
  const [order, setOrder] = useState("desc");
  const [operations, setOperations] = useState<TestOperations>({});
  const manuallyPolledOperations = useRef(new Set<string>());
  const [busy, setBusy] = useState<string>();
  const [error, setError] = useState<string>();

  const load = useCallback(async () => {
    const [nextStatus, nextPage] = await Promise.all([
      api.status(),
      api.nodes({ page: pageNumber, pageSize: PAGE_SIZE, search, sort, order })
    ]);
    setStatus(nextStatus);
    setPage(nextPage);
    setOperations((current) => {
      const next = { ...current };
      let changed = false;
      for (const node of nextPage.items) {
        const operation = next[node.id];
        if (
          node.latestTest !== undefined &&
          operation !== undefined &&
          !manuallyPolledOperations.current.has(operation.id)
        ) {
          delete next[node.id];
          changed = true;
        }
      }
      return changed ? next : current;
    });
  }, [order, pageNumber, search, sort]);

  useEffect(() => {
    void load().catch((reason: unknown) => setError(errorMessage(reason)));
    const events = new EventSource("/api/v1/events");
    let reloadTimer: number | undefined;
    const reloadSoon = () => {
      if (reloadTimer !== undefined) window.clearTimeout(reloadTimer);
      reloadTimer = window.setTimeout(() => {
        void load().catch((reason: unknown) => setError(errorMessage(reason)));
      }, 140);
    };
    const handleUpdate = (event: Event) => {
      if (event instanceof MessageEvent && typeof event.data === "string") {
        const appEvent = parseAppEvent(event.data);
        if (appEvent?.type === "test") {
          const { operationId, state } = appEvent.data;
          const terminal = state.state === "succeeded" || state.state === "failed";
          const manual = manuallyPolledOperations.current.has(operationId);
          if (terminal) manuallyPolledOperations.current.delete(operationId);
          setOperations((current) => {
            if (terminal && !manual) {
              if (current[state.nodeId]?.id !== operationId) return current;
              const next = { ...current };
              delete next[state.nodeId];
              return next;
            }
            return { ...current, [state.nodeId]: { id: operationId, state } };
          });
        }
      }
      reloadSoon();
    };
    events.addEventListener("update", handleUpdate);
    events.addEventListener("resync", reloadSoon);
    return () => {
      events.close();
      if (reloadTimer !== undefined) window.clearTimeout(reloadTimer);
    };
  }, [load]);

  useEffect(() => {
    const pending = Object.values(operations).filter(({ id, state }) =>
      manuallyPolledOperations.current.has(id) && isPending(state)
    );
    if (pending.length === 0) return;
    const timer = window.setInterval(() => {
      for (const operation of pending) {
        void api
          .testStatus(operation.id)
          .then((state) => {
            setOperations((current) => ({
              ...current,
              [state.nodeId]: { id: operation.id, state }
            }));
            if (!isPending(state)) {
              manuallyPolledOperations.current.delete(operation.id);
              void load().catch((reason: unknown) => setError(errorMessage(reason)));
            }
          })
          .catch((reason: unknown) => setError(errorMessage(reason)));
      }
    }, 1_500);
    return () => window.clearInterval(timer);
  }, [load, operations]);

  const runAction = async (key: string, action: () => Promise<unknown>) => {
    setBusy(key);
    setError(undefined);
    try {
      await action();
      await load();
    } catch (reason) {
      setError(errorMessage(reason));
    } finally {
      setBusy(undefined);
    }
  };

  return {
    status,
    page,
    draftSearch,
    sort,
    order,
    operations,
    busy,
    error,
    setDraftSearch,
    applySearch: () => {
      setPageNumberState(1);
      setSearch(draftSearch.trim());
    },
    changeSort: (value) => {
      setPageNumberState(1);
      setSort(value);
    },
    changeOrder: (value) => {
      setPageNumberState(1);
      setOrder(value);
    },
    setPageNumber: setPageNumberState,
    refresh: () => void runAction("refresh", api.refresh),
    disconnect: () => void runAction("disconnect", api.disconnect),
    connect: (nodeId) => void runAction(`connect:${nodeId}`, () => api.connect(nodeId)),
    test: (nodeId) => {
      void runAction(`test:${nodeId}`, async () => {
        const id = await api.startTest(nodeId);
        manuallyPolledOperations.current.add(id);
        setOperations((current) => ({
          ...current,
          [nodeId]: {
            id,
            state: { state: "queued", nodeId, queuedAt: new Date().toISOString() }
          }
        }));
      });
    },
    logout: () => {
      setBusy("logout");
      setError(undefined);
      void api.logout().then(onLoggedOut).catch((reason: unknown) => {
        setError(errorMessage(reason));
        setBusy(undefined);
      });
    }
  };
}

function isPending(state: TestState): boolean {
  return state.state === "queued" || state.state === "running";
}
