export type NodeAvailability =
  | "available"
  | "unsupportedProtocol"
  | "invalidConfig";

export interface IpPureResult {
  fraudScore: number;
  isResidential: boolean;
  isBroadcast: boolean;
  exitIp?: string;
}

export interface TestRecord {
  nodeId: string;
  result?: IpPureResult;
  durationMs: number;
  testedAt: string;
  error?: string;
}

export interface VpnNode {
  id: string;
  hostname: string;
  ip: string;
  score: number;
  pingMs: number | null;
  speedBps: number;
  countryLong: string;
  countryShort: string;
  sessions: number;
  uptimeMs: number;
  totalUsers: number;
  totalTrafficBytes: number;
  logType: string;
  operator: string;
  message: string;
  tcpPort: number | null;
  availability: NodeAvailability;
  latestTest?: TestRecord;
}

export interface NodesPage {
  items: VpnNode[];
  page: number;
  pageSize: number;
  total: number;
}

export type ConnectionState =
  | { state: "disconnected" }
  | { state: "connecting"; nodeId: string; workerId: string; since: string }
  | { state: "connected"; nodeId: string; workerId: string; since: string }
  | { state: "failed"; nodeId: string; message: string; at: string };

export type TestState =
  | { state: "queued"; nodeId: string; queuedAt: string }
  | { state: "running"; nodeId: string; startedAt: string }
  | { state: "succeeded"; nodeId: string; record: TestRecord }
  | { state: "failed"; nodeId: string; record: TestRecord };

export interface TestOperation {
  id: string;
  state: TestState;
}

export type TestOperations = Record<string, TestOperation>;

export interface StatusSnapshot {
  connection: ConnectionState;
  proxyReady: boolean;
  queuedTests: number;
  runningTests: number;
  upstreamState: "checking" | "ready" | "unreachable" | "authenticationFailed" | "netdUnavailable";
  lastRefresh?: {
    at: string;
    accepted: number;
    rejected: number;
    unsupported: number;
  };
  lanMode: boolean;
  tlsConfigured: boolean;
}

export interface SessionState {
  authenticated: boolean;
  csrfToken?: string;
}

export interface ApiFailure {
  error: {
    code: string;
    message: string;
  };
}

export type AppEvent =
  | { type: "connection"; data: ConnectionState }
  | { type: "test"; data: { operationId: string; state: TestState } }
  | {
      type: "nodesRefreshed";
      data: { accepted: number; rejected: number; at: string };
    }
  | { type: "refreshFailed"; data: { message: string; at: string } }
  | {
      type: "upstream";
      data: { state: StatusSnapshot["upstreamState"]; at: string };
    };
