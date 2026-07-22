import { fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";

import type { AutoConnectSettings } from "../types";
import { AutoConnectPanel } from "./AutoConnectPanel";

const settings: AutoConnectSettings = {
  config: {
    enabled: false,
    ipType: "any",
    residential: "any"
  },
  regions: [
    { code: "JP", name: "Japan" },
    { code: "US", name: "United States" }
  ]
};

describe("AutoConnectPanel", () => {
  it("submits the enabled region and IP classification filters", () => {
    const onSave = vi.fn();
    render(
      <AutoConnectPanel settings={settings} busy={undefined} onSave={onSave} />
    );

    fireEvent.click(screen.getByRole("checkbox", { name: /启用自动连接策略/ }));
    fireEvent.change(screen.getByLabelText("地区"), { target: { value: "JP" } });
    fireEvent.change(screen.getByLabelText("IP 类型"), { target: { value: "native" } });
    fireEvent.change(screen.getByLabelText("住宅属性"), {
      target: { value: "residential" }
    });
    fireEvent.click(screen.getByRole("button", { name: "保存策略" }));

    expect(onSave).toHaveBeenCalledWith({
      enabled: true,
      region: "JP",
      ipType: "native",
      residential: "residential"
    });
  });
});
