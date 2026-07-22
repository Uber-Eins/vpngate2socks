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

describe("NodeTable", () => {
  it("renders active state and all IPPure risk fields", () => {
    render(
      <NodeTable
        nodes={[node]}
        activeNodeId={node.id}
        operations={{}}
        busy={undefined}
        onConnect={vi.fn()}
        onTest={vi.fn()}
      />
    );

    expect(screen.getByText("Japan")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "已连接" })).toBeDisabled();
    expect(screen.getByRole("meter", { name: "欺诈分 7" })).toBeInTheDocument();
    expect(screen.getByText("住宅出口")).toBeInTheDocument();
    expect(screen.getByText("非广播网络")).toBeInTheDocument();
    expect(screen.getByText("8.8.8.8")).toBeInTheDocument();
    expect(screen.getByText(/420 ms/)).toBeInTheDocument();
  });

  it("shows isolated progress without changing the connect action", () => {
    render(
      <NodeTable
        nodes={[node]}
        activeNodeId={undefined}
        operations={{
          [node.id]: {
            id: "operation",
            state: {
              state: "running",
              nodeId: node.id,
              startedAt: "2026-07-22T00:00:00Z"
            }
          }
        }}
        busy={undefined}
        onConnect={vi.fn()}
        onTest={vi.fn()}
      />
    );

    expect(screen.getAllByText("测试中").length).toBeGreaterThan(0);
    expect(screen.getByRole("button", { name: "连接" })).toBeEnabled();
  });

  it("marks an eligible node without data as awaiting automatic testing", () => {
    const untested = { ...node };
    delete untested.latestTest;
    render(
      <NodeTable
        nodes={[untested]}
        activeNodeId={undefined}
        operations={{}}
        busy={undefined}
        onConnect={vi.fn()}
        onTest={vi.fn()}
      />
    );

    expect(screen.getByText("等待自动检测")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "优先检测" })).toBeEnabled();
  });
});
