import { render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";

import { NodeTable } from "./App";
import type { VpnNode } from "./types";

const node: VpnNode = {
  id: "a".repeat(64), hostname: "vpn.example", ip: "1.2.3.4", score: 900,
  pingMs: 24, speedBps: 12_000_000, countryLong: "Japan", countryShort: "JP",
  sessions: 8, uptimeMs: 1_000, totalUsers: 50, totalTrafficBytes: 500,
  logType: "2weeks", operator: "volunteer", message: "fast", tcpPort: 443,
  availability: "available",
  latestTest: {
    nodeId: "a".repeat(64), durationMs: 420, testedAt: "2026-07-22T00:00:00Z",
    result: { fraudScore: 7, isResidential: true, isBroadcast: false, exitIp: "8.8.8.8" }
  }
};

describe("NodeTable", () => {
  it("renders region, active state, and all required IPPure risk fields", () => {
    render(
      <NodeTable
        nodes={[node]}
        activeNodeId={node.id}
        operations={{}}
        onConnect={vi.fn()}
        onTest={vi.fn()}
      />
    );
    expect(screen.getByText("Japan")).toBeInTheDocument();
    expect(screen.getByText("已连接")).toBeDisabled();
    expect(screen.getByText(/欺诈分/)).toHaveTextContent("7");
    expect(screen.getByText("住宅出口")).toBeInTheDocument();
    expect(screen.getByText("非广播网络")).toBeInTheDocument();
    expect(screen.getByText("8.8.8.8")).toBeInTheDocument();
    expect(screen.getByText(/420 ms/)).toBeInTheDocument();
  });

  it("shows isolated progress without changing the connect action", () => {
    render(
      <NodeTable
        nodes={[node]}
        operations={{
          [node.id]: {
            id: "operation",
            state: { state: "running", nodeId: node.id, startedAt: "2026-07-22T00:00:00Z" }
          }
        }}
        onConnect={vi.fn()}
        onTest={vi.fn()}
      />
    );
    expect(screen.getAllByText("测试中").length).toBeGreaterThan(0);
    expect(screen.getByText("连接")).toBeEnabled();
  });
});
