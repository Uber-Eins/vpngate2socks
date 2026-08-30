import { useCallback, useEffect, useRef, useState } from "react";

import * as api from "../api";
import { parseAppEvent } from "../events/parseAppEvent";
import type { Toast, ToastTone } from "../ui/Toasts";
import type {
  AutoConnectConfig,
  AutoConnectSettings,
  NodeQuery,
  NodesPage,
  SortKey,
  StatusSnapshot,
  TestOperations,
  TestState
} from "../types";
import { errorMessage } from "../utils/errorMessage";

const PAGE_SIZE = 50;
const EMPTY_PAGE: NodesPage = { items: [], page: 1, pageSize: PAGE_SIZE, total: 0 };
const TOAST_TIMEOUT = 6_000;

export const DEFAULT_QUERY: NodeQuery = {
  search: "",
  region: "",
  ipType: "any",
  residential: "any",
  availability: "any",
  sort: "score",
  order: "desc"
};

/** Sort keys where a smaller number is the better node. */
const ASCENDING_BY_DEFAULT: SortKey[] = ["ping", "fraud"];

export interface ConsoleModel {
  status: StatusSnapshot | undefined;
  autoConnect: AutoConnectSettings | undefined;
  page: NodesPage;
  pageNumber: number;
  query: NodeQuery;
  draftSearch: string;
  operations: TestOperations;
  busy: string | undefined;
  error: string | undefined;
  loadingNodes: boolean;
  live: boolean;
  toasts: Toast[];
  dismissToast: (id: number) => void;
  setDraftSearch: (value: string) => void;
  applySearch: () => void;
  setFilter: <K extends keyof NodeQuery>(key: K, value: NodeQuery[K]) => void;
  toggleSort: (key: SortKey) => void;
  resetFilters: () => void;
  setPageNumber: (value: number) => void;
  refresh: () => void;
  disconnect: () => void;
  connect: (nodeId: string) => void;
  test: (nodeId: string) => void;
  saveAutoConnect: (config: AutoConnectConfig) => void;
  logout: () => void;
}

export function useConsole(onLoggedOut: () => void, strings: ConsoleStrings): ConsoleModel {
  const [status, setStatus] = useState<StatusSnapshot>();
  const [autoConnect, setAutoConnect] = useState<AutoConnectSettings>();
  const [page, setPage] = useState(EMPTY_PAGE);
  const [pageNumber, setPageNumberState] = useState(1);
  const [query, setQuery] = useState<NodeQuery>(DEFAULT_QUERY);
  const [draftSearch, setDraftSearch] = useState("");
  const [operations, setOperations] = useState<TestOperations>({});
  const [busy, setBusy] = useState<string>();
  const [error, setError] = useState<string>();
  const [loadingNodes, setLoadingNodes] = useState(true);
  const [live, setLive] = useState(false);
  const [toasts, setToasts] = useState<Toast[]>([]);
  const manuallyPolledOperations = useRef(new Set<string>());
  const toastId = useRef(0);

  const pushToast = useCallback((tone: ToastTone, title: string, detail?: string) => {
    toastId.current += 1;
    const id = toastId.current;
    setToasts((current) => [
      ...current.slice(-2),
      { id, tone, title, ...(detail === undefined ? {} : { detail }) }
    ]);
    window.setTimeout(
      () => setToasts((current) => current.filter((toast) => toast.id !== id)),
      TOAST_TIMEOUT
    );
  }, []);

  const load = useCallback(async () => {
    const [nextStatus, nextPage, nextAutoConnect] = await Promise.all([
      api.status(),
      api.nodes({ ...query, page: pageNumber, pageSize: PAGE_SIZE }),
      api.autoConnection()
    ]);
    setStatus(nextStatus);
    setPage(nextPage);
    setAutoConnect(nextAutoConnect);
    setError(undefined);
    // An automatic test that finished server-side is already reflected in the node
    // row, so its transient operation entry is dropped unless this tab started it.
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
  }, [pageNumber, query]);

  const loadRef = useRef(load);
  useEffect(() => {
    loadRef.current = load;
  }, [load]);

  useEffect(() => {
    setLoadingNodes(true);
    void load()
      .catch((reason: unknown) => setError(errorMessage(reason)))
      .finally(() => setLoadingNodes(false));
  }, [load]);

  // The event stream is opened once; filter changes must not tear it down.
  useEffect(() => {
    const events = new EventSource("/api/v1/events");
    let reloadTimer: number | undefined;
    const reloadSoon = () => {
      if (reloadTimer !== undefined) window.clearTimeout(reloadTimer);
      reloadTimer = window.setTimeout(() => {
        void loadRef.current().catch((reason: unknown) => setError(errorMessage(reason)));
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
    events.addEventListener("open", () => setLive(true));
    events.addEventListener("error", () => setLive(false));
    events.addEventListener("update", handleUpdate);
    events.addEventListener("resync", reloadSoon);
    return () => {
      events.close();
      if (reloadTimer !== undefined) window.clearTimeout(reloadTimer);
    };
  }, []);

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
              void loadRef.current().catch((reason: unknown) => setError(errorMessage(reason)));
            }
          })
          .catch((reason: unknown) => setError(errorMessage(reason)));
      }
    }, 1_500);
    return () => window.clearInterval(timer);
  }, [operations]);

  const runAction = async (
    key: string,
    action: () => Promise<unknown>,
    success?: { tone: ToastTone; title: string }
  ) => {
    setBusy(key);
    try {
      await action();
      await load();
      if (success !== undefined) pushToast(success.tone, success.title);
    } catch (reason) {
      pushToast("danger", strings.toastFailed, errorMessage(reason));
    } finally {
      setBusy(undefined);
    }
  };

  const updateQuery = (patch: Partial<NodeQuery>) => {
    setPageNumberState(1);
    setQuery((current) => ({ ...current, ...patch }));
  };

  return {
    status,
    autoConnect,
    page,
    pageNumber,
    query,
    draftSearch,
    operations,
    busy,
    error,
    loadingNodes,
    live,
    toasts,
    dismissToast: (id) => setToasts((current) => current.filter((toast) => toast.id !== id)),
    setDraftSearch,
    applySearch: () => updateQuery({ search: draftSearch.trim() }),
    setFilter: (key, value) => updateQuery({ [key]: value } as Partial<NodeQuery>),
    toggleSort: (key) =>
      updateQuery(
        key === query.sort
          ? { order: query.order === "asc" ? "desc" : "asc" }
          : { sort: key, order: ASCENDING_BY_DEFAULT.includes(key) ? "asc" : "desc" }
      ),
    resetFilters: () => {
      setDraftSearch("");
      setPageNumberState(1);
      setQuery((current) => ({ ...DEFAULT_QUERY, sort: current.sort, order: current.order }));
    },
    setPageNumber: setPageNumberState,
    refresh: () =>
      void runAction("refresh", api.refresh, {
        tone: "success",
        title: strings.toastRefreshDone
      }),
    disconnect: () =>
      void runAction("disconnect", api.disconnect, {
        tone: "info",
        title: strings.toastDisconnected
      }),
    connect: (nodeId) =>
      void runAction(`connect:${nodeId}`, () => api.connect(nodeId), {
        tone: "info",
        title: strings.toastConnecting
      }),
    test: (nodeId) => {
      void runAction(
        `test:${nodeId}`,
        async () => {
          const id = await api.startTest(nodeId);
          manuallyPolledOperations.current.add(id);
          setOperations((current) => ({
            ...current,
            [nodeId]: {
              id,
              state: { state: "queued", nodeId, queuedAt: new Date().toISOString() }
            }
          }));
        },
        { tone: "info", title: strings.toastTestQueued }
      );
    },
    saveAutoConnect: (config) => {
      void runAction(
        "autoConnect",
        async () => {
          const settings = await api.updateAutoConnection(config);
          setAutoConnect(settings);
        },
        { tone: "success", title: strings.toastPolicySaved }
      );
    },
    logout: () => {
      setBusy("logout");
      void api.logout().then(onLoggedOut).catch((reason: unknown) => {
        pushToast("danger", strings.toastFailed, errorMessage(reason));
        setBusy(undefined);
      });
    }
  };
}

export interface ConsoleStrings {
  toastFailed: string;
  toastConnecting: string;
  toastDisconnected: string;
  toastRefreshDone: string;
  toastTestQueued: string;
  toastPolicySaved: string;
}

function isPending(state: TestState): boolean {
  return state.state === "queued" || state.state === "running";
}
