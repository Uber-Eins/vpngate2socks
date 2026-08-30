import { render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";

import type { VpnNode } from "../types";
import { NodeTable } from "./NodeTable";

const node: VpnNode = {
  id: "a".repeat(64),
  hostname: "vpn.example",
  ip: "1.2.3.4",
  score: 900,
  pingMs: 24,
  speedBps: 12_000_000,
  countryLong: "Japan",
  countryShort: "JP",
  sessions: 8,
  uptimeMs: 1_000,
  totalUsers: 50,
  totalTrafficBytes: 500,
  logType: "2weeks",
  operator: "volunteer",
  message: "fast",
  tcpPort: 443,
  availability: "available",
  latestTest: {
    nodeId: "a".repeat(64),
    durationMs: 420,
    testedAt: "2026-07-22T00:00:00Z",
    result: {
      fraudScore: 7,
      isResidential: true,
      isBroadcast: false,
      exitIp: "8.8.8.8"
    }
  }
};

function renderTable(overrides: Partial<Parameters<typeof NodeTable>[0]> = {}) {
  const props = {
    nodes: [node],
    activeNodeId: undefined,
    operations: {},
    busy: undefined,
    loading: false,
    sort: "score" as const,
    order: "desc" as const,
    onSort: vi.fn(),
    onConnect: vi.fn(),
    onTest: vi.fn(),
    onInspect: vi.fn(),
    ...overrides
  };
  render(<NodeTable {...props} />);
  return props;
}

describe("NodeTable", () => {
  it("renders every IPPure risk field for a tested node", () => {
    renderTable();

    expect(screen.getByText("Japan")).toBeInTheDocument();
    expect(screen.getByRole("meter", { name: "欺诈分 7" })).toBeInTheDocument();
    expect(screen.getByText("住宅")).toBeInTheDocument();
    expect(screen.getByText("非广播")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "重新检测" })).toBeEnabled();
  });

  it("disables connecting to the node that is already active", () => {
    renderTable({ activeNodeId: node.id });

    expect(screen.getByRole("button", { name: "已连接" })).toBeDisabled();
  });

  it("shows isolated test progress without blocking the connect action", () => {
    renderTable({
      operations: {
        [node.id]: {
          id: "operation",
          state: { state: "running", nodeId: node.id, startedAt: "2026-07-22T00:00:00Z" }
        }
      }
    });

    expect(screen.getByText("检测中")).toBeInTheDocument();
    expect(screen.queryByRole("meter")).not.toBeInTheDocument();
    expect(screen.getByRole("button", { name: "连接" })).toBeEnabled();
  });

  it("marks an eligible node without data as awaiting automatic testing", () => {
    const untested = { ...node };
    delete untested.latestTest;
    renderTable({ nodes: [untested] });

    expect(screen.getByText("等待自动检测")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "优先检测" })).toBeEnabled();
  });

  it("refuses to connect to a node this version cannot use", () => {
    renderTable({ nodes: [{ ...node, availability: "unsupportedProtocol" }] });

    expect(screen.getByRole("button", { name: "v1 不可用" })).toBeDisabled();
  });

  it("reports the sorted column and direction to assistive technology", () => {
    renderTable({ sort: "fraud", order: "asc" });

    expect(screen.getByRole("columnheader", { name: /IPPure 风险/ })).toHaveAttribute(
      "aria-sort",
      "ascending"
    );
    expect(screen.getByRole("columnheader", { name: /延迟/ })).toHaveAttribute(
      "aria-sort",
      "none"
    );
  });

  it("opens the detail view from the node name", () => {
    const props = renderTable();

    screen.getByRole("button", { name: /vpn.example/ }).click();

    expect(props.onInspect).toHaveBeenCalledWith(node);
  });
});
