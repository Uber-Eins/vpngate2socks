import type { AppEvent } from "../types";

export function parseAppEvent(raw: string): AppEvent | undefined {
  try {
    const value: unknown = JSON.parse(raw);
    if (typeof value !== "object" || value === null || !("type" in value) || !("data" in value)) {
      return undefined;
    }
    const type = value.type;
    if (
      type !== "connection" &&
      type !== "autoConnection" &&
      type !== "test" &&
      type !== "nodesRefreshed" &&
      type !== "refreshFailed" &&
      type !== "upstream"
    ) {
      return undefined;
    }
    return value as AppEvent;
  } catch {
    return undefined;
  }
}
