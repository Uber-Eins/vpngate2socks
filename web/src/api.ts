import type {
  ApiFailure,
  AutoConnectConfig,
  AutoConnectSettings,
  ConnectionState,
  NodeQuery,
  NodesPage,
  SessionState,
  StatusSnapshot,
  TestState
} from "./types";

let csrfToken: string | undefined;

export class ApiError extends Error {
  readonly code: string;
  readonly status: number;

  constructor(status: number, code: string, message: string) {
    super(message);
    this.name = "ApiError";
    this.code = code;
    this.status = status;
  }
}

async function request<T>(path: string, init: RequestInit = {}): Promise<T> {
  const method = init.method?.toUpperCase() ?? "GET";
  const headers = new Headers(init.headers);
  headers.set("accept", "application/json");
  if (init.body !== undefined) {
    headers.set("content-type", "application/json");
  }
  if (!["GET", "HEAD", "OPTIONS"].includes(method) && csrfToken !== undefined) {
    headers.set("x-csrf-token", csrfToken);
  }
  const response = await fetch(path, { ...init, headers, credentials: "same-origin" });
  if (!response.ok) {
    let failure: ApiFailure | undefined;
    try {
      failure = (await response.json()) as ApiFailure;
    } catch {
      failure = undefined;
    }
    throw new ApiError(
      response.status,
      failure?.error.code ?? "requestFailed",
      failure?.error.message ?? `HTTP ${response.status}`
    );
  }
  if (response.status === 204) {
    return undefined as T;
  }
  return (await response.json()) as T;
}

export async function session(): Promise<SessionState> {
  const result = await request<SessionState>("/api/v1/auth/session");
  csrfToken = result.csrfToken;
  return result;
}

export async function login(username: string, password: string): Promise<SessionState> {
  const result = await request<SessionState>("/api/v1/auth/login", {
    method: "POST",
    body: JSON.stringify({ username, password })
  });
  csrfToken = result.csrfToken;
  return result;
}

export async function logout(): Promise<void> {
  await request<void>("/api/v1/auth/session", { method: "DELETE" });
  csrfToken = undefined;
}

export function nodes(
  params: NodeQuery & { page: number; pageSize: number }
): Promise<NodesPage> {
  const query = new URLSearchParams({
    page: String(params.page),
    pageSize: String(params.pageSize),
    search: params.search,
    sort: params.sort,
    order: params.order,
    ipType: params.ipType,
    residential: params.residential,
    availability: params.availability
  });
  if (params.region !== "") {
    query.set("region", params.region);
  }
  return request(`/api/v1/nodes?${query.toString()}`);
}

export const status = (): Promise<StatusSnapshot> => request("/api/v1/status");

export const refresh = (): Promise<void> =>
  request("/api/v1/nodes/refresh", { method: "POST" });

export const connect = (nodeId: string): Promise<ConnectionState> =>
  request("/api/v1/connection", {
    method: "PUT",
    body: JSON.stringify({ nodeId })
  });

export const disconnect = (): Promise<ConnectionState> =>
  request("/api/v1/connection", { method: "DELETE" });

export const autoConnection = (): Promise<AutoConnectSettings> =>
  request("/api/v1/auto-connection");

export const updateAutoConnection = (
  config: AutoConnectConfig
): Promise<AutoConnectSettings> =>
  request("/api/v1/auto-connection", {
    method: "PUT",
    body: JSON.stringify(config)
  });

export async function startTest(nodeId: string): Promise<string> {
  const result = await request<{ operationId: string }>(
    `/api/v1/nodes/${encodeURIComponent(nodeId)}/tests`,
    { method: "POST" }
  );
  return result.operationId;
}

export const testStatus = (operationId: string): Promise<TestState> =>
  request(`/api/v1/tests/${encodeURIComponent(operationId)}`);
