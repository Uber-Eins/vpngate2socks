import { describe, expect, it } from "vitest";

import { parseAppEvent } from "./parseAppEvent";

describe("parseAppEvent", () => {
  it("accepts a test event emitted by the Rust SSE endpoint", () => {
    const event = parseAppEvent(JSON.stringify({
      type: "test",
      data: {
        operationId: "operation",
        state: {
          state: "queued",
          nodeId: "a".repeat(64),
          queuedAt: "2026-07-22T00:00:00Z"
        }
      }
    }));

    expect(event?.type).toBe("test");
    if (event?.type === "test") {
      expect(event.data.state.state).toBe("queued");
    }
  });

  it("rejects malformed or unknown event envelopes", () => {
    expect(parseAppEvent("not json")).toBeUndefined();
    expect(parseAppEvent('{"type":"unknown","data":{}}')).toBeUndefined();
    expect(parseAppEvent('{"type":"test"}')).toBeUndefined();
  });

  it("accepts automatic connection configuration events", () => {
    const event = parseAppEvent(JSON.stringify({
      type: "autoConnection",
      data: {
        enabled: true,
        region: "JP",
        ipType: "native",
        residential: "residential"
      }
    }));

    expect(event?.type).toBe("autoConnection");
  });
});
