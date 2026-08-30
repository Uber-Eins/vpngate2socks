import { zhCN as t } from "../i18n";
import type { DotTone } from "../ui/Badge";
import type { ConnectionState, NodeAvailability, StatusSnapshot, UpstreamState } from "../types";

export function connectionLabel(connection: ConnectionState | undefined): string {
  switch (connection?.state) {
    case "connected":
      return t.connected;
    case "connecting":
      return t.connecting;
    case "failed":
      return t.failed;
    default:
      return t.disconnected;
  }
}

export function connectionTone(status: StatusSnapshot | undefined): DotTone {
  if (status === undefined) return "idle";
  if (status.proxyReady) return "online";
  if (status.connection.state === "connecting") return "busy";
  if (status.connection.state === "failed") return "danger";
  return "idle";
}

export function upstreamLabel(state: UpstreamState | undefined): string {
  switch (state) {
    case "ready":
      return t.upstreamReady;
    case "checking":
      return t.upstreamChecking;
    case "unreachable":
      return t.upstreamUnreachable;
    case "authenticationFailed":
      return t.upstreamAuthenticationFailed;
    case "netdUnavailable":
      return t.upstreamNetdUnavailable;
    default:
      return "—";
  }
}

export function upstreamTone(state: UpstreamState | undefined): DotTone {
  switch (state) {
    case "ready":
      return "online";
    case "checking":
      return "busy";
    case undefined:
      return "idle";
    default:
      return "danger";
  }
}

export function availabilityLabel(availability: NodeAvailability): string {
  switch (availability) {
    case "available":
      return t.available;
    case "unsupportedProtocol":
      return t.unsupportedProtocol;
    case "invalidConfig":
      return t.invalidConfig;
  }
}
