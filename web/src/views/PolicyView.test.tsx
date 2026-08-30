import { fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";

import type { AutoConnectSettings } from "../types";
import { PolicyView } from "./PolicyView";

const settings: AutoConnectSettings = {
  config: { enabled: false, ipType: "any", residential: "any" },
  regions: [
    { code: "JP", name: "Japan" },
    { code: "US", name: "United States" }
  ]
};

describe("PolicyView", () => {
  it("submits the enabled region and IP classification filters", () => {
    const onSave = vi.fn();
    render(<PolicyView settings={settings} busy={undefined} onSave={onSave} />);

    fireEvent.click(screen.getByRole("switch", { name: /启用自动连线/ }));
    fireEvent.change(screen.getByLabelText("地区"), { target: { value: "JP" } });
    fireEvent.change(screen.getByLabelText("IP 类型"), { target: { value: "native" } });
    fireEvent.change(screen.getByLabelText("住宅属性"), { target: { value: "residential" } });
    fireEvent.click(screen.getByRole("button", { name: "保存策略" }));

    expect(onSave).toHaveBeenCalledWith({
      enabled: true,
      region: "JP",
      ipType: "native",
      residential: "residential"
    });
  });

  it("omits the region entirely when the filter is cleared", () => {
    const onSave = vi.fn();
    render(
      <PolicyView
        settings={{ ...settings, config: { ...settings.config, region: "JP" } }}
        busy={undefined}
        onSave={onSave}
      />
    );

    fireEvent.change(screen.getByLabelText("地区"), { target: { value: "" } });
    fireEvent.click(screen.getByRole("button", { name: "保存策略" }));

    expect(onSave).toHaveBeenCalledWith({
      enabled: false,
      ipType: "any",
      residential: "any"
    });
  });

  it("keeps saving disabled until the draft differs from the stored policy", () => {
    render(<PolicyView settings={settings} busy={undefined} onSave={vi.fn()} />);

    expect(screen.getByRole("button", { name: "保存策略" })).toBeDisabled();

    fireEvent.click(screen.getByRole("switch", { name: /启用自动连线/ }));

    expect(screen.getByRole("button", { name: "保存策略" })).toBeEnabled();
    expect(screen.getByText("有未保存的更改")).toBeInTheDocument();
  });
});
