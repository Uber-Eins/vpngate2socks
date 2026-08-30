/*
 * Development-only in-browser stand-in for the Rust control plane.
 *
 * The real backend needs netd, /dev/net/tun and an upstream SOCKS5, which makes
 * iterating on the UI awkward. `npm run dev:mock` loads this module instead; it
 * intercepts fetch and EventSource and mirrors the server's filtering, sorting and
 * paging rules so the console can be exercised end to end. It is behind
 * `import.meta.env.DEV` and is dropped from production builds.
 */
import type {
  AutoConnectConfig,
  ConnectionState,
  NodesPage,
  StatusSnapshot,
  VpnNode
} from "../types";
import { buildNodes } from "./data";

const NODES = buildNodes(180);

let connection: ConnectionState = {
  state: "connected",
  nodeId: NODES[3]?.id ?? "",
  workerId: "0f7c9a41-2b6e-4c33-9f01-2c5d9a7b1e88",
  since: new Date(Date.now() - 4_912_000).toISOString()
};
let autoConnect: AutoConnectConfig = { enabled: true, region: "JP", ipType: "native", residential: "any" };
let queuedTests = 3;
let runningTests = 1;

const listeners = new Set<(event: MessageEvent) => void>();

function emit(): void {
  for (const listener of listeners) {
    listener(new MessageEvent("update", { data: JSON.stringify({ type: "connection", data: connection }) }));
  }
}

function status(): StatusSnapshot {
  const activeId = connection.state === "disconnected" ? undefined : connection.nodeId;
  const activeNode = NODES.find((node) => node.id === activeId);
  return {
    connection,
    ...(activeNode === undefined ? {} : { activeNode }),
    proxyReady: connection.state === "connected",
    queuedTests,
    runningTests,
    upstreamState: "ready",
    lastRefresh: {
      at: new Date(Date.now() - 214_000).toISOString(),
      accepted: NODES.filter((node) => node.availability === "available").length,
      rejected: 46,
      unsupported: NODES.filter((node) => node.availability !== "available").length
    },
    lanMode: true,
    tlsConfigured: false
  };
}

function page(url: URL): NodesPage {
  const pageNumber = Number(url.searchParams.get("page") ?? "1");
  const pageSize = Number(url.searchParams.get("pageSize") ?? "50");
  const search = (url.searchParams.get("search") ?? "").toLowerCase();
  const region = url.searchParams.get("region");
  const ipType = url.searchParams.get("ipType") ?? "any";
  const residential = url.searchParams.get("residential") ?? "any";
  const availability = url.searchParams.get("availability") ?? "any";
  const sort = url.searchParams.get("sort") ?? "score";
  const order = url.searchParams.get("order") ?? "desc";

  const filtered = NODES.filter((node) => {
    if (availability === "available" && node.availability !== "available") return false;
    if (region !== null && node.countryShort.toLowerCase() !== region.toLowerCase()) return false;
    if (ipType !== "any" || residential !== "any") {
      const result = node.latestTest?.result;
      if (result === undefined) return false;
      if (ipType === "native" && result.isBroadcast) return false;
      if (ipType === "broadcast" && !result.isBroadcast) return false;
      if (residential === "residential" && !result.isResidential) return false;
      if (residential === "nonResidential" && result.isResidential) return false;
    }
    if (search === "") return true;
    return [node.hostname, node.ip, node.countryLong, node.countryShort, node.operator, node.message]
      .some((field) => field.toLowerCase().includes(search));
  });

  filtered.sort((left, right) => {
    const missing = (node: VpnNode) =>
      sort === "ping" ? node.pingMs === null : sort === "fraud" && node.latestTest?.result === undefined;
    if (missing(left) !== missing(right)) return missing(left) ? 1 : -1;
    const direction = order === "asc" ? 1 : -1;
    return direction * (value(left, sort) - value(right, sort));
  });

  const offset = (pageNumber - 1) * pageSize;
  return {
    items: filtered.slice(offset, offset + pageSize),
    page: pageNumber,
    pageSize,
    total: filtered.length
  };
}

function value(node: VpnNode, sort: string): number {
  switch (sort) {
    case "ping":
      return node.pingMs ?? Number.MAX_SAFE_INTEGER;
    case "speed":
      return node.speedBps;
    case "sessions":
      return node.sessions;
    case "fraud":
      return node.latestTest?.result?.fraudScore ?? Number.MAX_SAFE_INTEGER;
    default:
      return node.score;
  }
}

function regions() {
  const unique = new Map<string, string>();
  for (const node of NODES) {
    if (node.availability === "available") unique.set(node.countryShort, node.countryLong);
  }
  return [...unique.entries()]
    .sort(([left], [right]) => left.localeCompare(right))
    .map(([code, name]) => ({ code, name }));
}

const originalFetch = window.fetch.bind(window);

window.fetch = async (input, init) => {
  const url = new URL(
    typeof input === "string" ? input : input instanceof URL ? input.href : input.url,
    window.location.origin
  );
  const method = (init?.method ?? "GET").toUpperCase();
  await new Promise((resolve) => window.setTimeout(resolve, 120));

  const json = (body: unknown) =>
    new Response(JSON.stringify(body), {
      status: 200,
      headers: { "content-type": "application/json" }
    });

  if (url.pathname === "/api/v1/auth/session" && method === "GET") {
    // `?login` starts on the sign-in screen instead of an authenticated session.
    return json(
      window.location.search.includes("login")
        ? { authenticated: false }
        : { authenticated: true, csrfToken: "mock-csrf-token" }
    );
  }
  if (url.pathname === "/api/v1/auth/login") {
    return json({ authenticated: true, csrfToken: "mock-csrf-token" });
  }
  if (url.pathname === "/api/v1/status") return json(status());
  if (url.pathname === "/api/v1/nodes") return json(page(url));
  if (url.pathname === "/api/v1/nodes/refresh") return json({});
  if (url.pathname === "/api/v1/auto-connection") {
    if (method === "PUT") autoConnect = JSON.parse(String(init?.body)) as AutoConnectConfig;
    return json({ config: autoConnect, regions: regions() });
  }
  if (url.pathname === "/api/v1/connection") {
    if (method === "DELETE") {
      connection = { state: "disconnected" };
    } else {
      const body = JSON.parse(String(init?.body)) as { nodeId: string };
      connection = {
        state: "connected",
        nodeId: body.nodeId,
        workerId: "3a1e77c0-91b4-4e2a-8f55-6d0b2a4c1f97",
        since: new Date().toISOString()
      };
    }
    emit();
    return json(connection);
  }
  if (url.pathname.endsWith("/tests") && method === "POST") {
    queuedTests += 1;
    return json({ operationId: crypto.randomUUID() });
  }
  return originalFetch(input, init);
};

class MockEventSource extends EventTarget {
  constructor(_url: string) {
    super();
    const forward = (event: MessageEvent) => this.dispatchEvent(event);
    listeners.add(forward);
    window.setTimeout(() => this.dispatchEvent(new Event("open")), 30);
  }

  close(): void {
    listeners.clear();
  }
}

Object.defineProperty(window, "EventSource", { writable: true, value: MockEventSource });
