import { fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";

import { DEFAULT_QUERY } from "../state/useConsole";
import type { NodeQuery } from "../types";
import { NodeFilters } from "./NodeFilters";

function renderFilters(query: NodeQuery = DEFAULT_QUERY) {
  const props = {
    query,
    draftSearch: "",
    regions: [{ code: "JP", name: "Japan" }],
    busy: false,
    onDraftSearch: vi.fn(),
    onSearch: vi.fn(),
    onFilter: vi.fn(),
    onReset: vi.fn()
  };
  render(<NodeFilters {...props} />);
  return props;
}

describe("NodeFilters", () => {
  it("reports a changed region to the caller", () => {
    const props = renderFilters();

    fireEvent.change(screen.getByLabelText("地区"), { target: { value: "JP" } });

    expect(props.onFilter).toHaveBeenCalledWith("region", "JP");
  });

  it("hides the reset action while the query is untouched", () => {
    renderFilters();

    expect(screen.queryByRole("button", { name: "清除筛选" })).not.toBeInTheDocument();
  });

  it("offers a reset once a filter is set", () => {
    renderFilters({ ...DEFAULT_QUERY, availability: "available" });

    expect(screen.getByRole("button", { name: "清除筛选" })).toBeInTheDocument();
  });

  it("warns that classification filters drop untested nodes", () => {
    renderFilters({ ...DEFAULT_QUERY, ipType: "native" });

    expect(
      screen.getByText(/只保留已完成 IPPure 检测的节点/)
    ).toBeInTheDocument();
  });
});
