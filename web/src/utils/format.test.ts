import { describe, expect, it } from "vitest";

import { formatBits, formatBytes, formatDuration, formatElapsed } from "./format";

describe("formatBits", () => {
  it("converts the VPN Gate bytes-per-second figure to bits", () => {
    expect(formatBits(12_000_000)).toBe("96.0 Mbps");
    expect(formatBits(2_000_000_000)).toBe("16.0 Gbps");
    expect(formatBits(1_000)).toBe("8 Kbps");
  });
});

describe("formatBytes", () => {
  it("steps through binary units and keeps whole bytes exact", () => {
    expect(formatBytes(512)).toBe("512 B");
    expect(formatBytes(1024)).toBe("1.0 KiB");
    expect(formatBytes(1_099_511_627_776)).toBe("1.0 TiB");
  });
});

describe("formatDuration", () => {
  it("drops the smallest unit once a larger one is present", () => {
    expect(formatDuration(45_000)).toBe("45 秒");
    expect(formatDuration(125_000)).toBe("2 分 05 秒");
    expect(formatDuration(3_900_000)).toBe("1 小时 5 分");
    expect(formatDuration(272_000_000)).toBe("3 天 3 小时");
  });

  it("never reports negative time", () => {
    expect(formatDuration(-5_000)).toBe("0 秒");
  });
});

describe("formatElapsed", () => {
  it("measures against the supplied clock", () => {
    const now = Date.parse("2026-08-29T12:00:00Z");
    expect(formatElapsed("2026-08-29T11:00:00Z", now)).toBe("1 小时 0 分");
  });

  it("falls back to a placeholder for an unparsable timestamp", () => {
    expect(formatElapsed("not-a-date", Date.now())).toBe("—");
  });
});
