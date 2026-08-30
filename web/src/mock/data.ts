import type { TestRecord, VpnNode } from "../types";

const REGIONS = [
  ["JP", "Japan"],
  ["KR", "Korea Republic of"],
  ["US", "United States"],
  ["TW", "Taiwan"],
  ["TH", "Thailand"],
  ["VN", "Viet Nam"],
  ["RU", "Russian Federation"],
  ["DE", "Germany"],
  ["GB", "United Kingdom"],
  ["CA", "Canada"]
] as const;

const OPERATORS = [
  "Daiyuu Nobori, Japan",
  "opengw volunteer",
  "SoftEther VPN Project",
  "anonymous",
  ""
];

const LOG_TYPES = ["2weeks", "1month", "no", "unknown"];

/** Deterministic pseudo-random source so reloads produce a stable data set. */
function random(seed: number): () => number {
  let state = seed;
  return () => {
    state = (state * 1_664_525 + 1_013_904_223) % 4_294_967_296;
    return state / 4_294_967_296;
  };
}

function hex(value: number): string {
  return value.toString(16).padStart(64, "0");
}

export function buildNodes(count: number): VpnNode[] {
  const next = random(20_260_829);
  const nodes: VpnNode[] = [];
  for (let index = 0; index < count; index += 1) {
    const [countryShort, countryLong] = REGIONS[Math.floor(next() * REGIONS.length)] ?? REGIONS[0];
    const roll = next();
    const availability = roll > 0.9
      ? (roll > 0.96 ? "invalidConfig" : "unsupportedProtocol")
      : "available";
    const id = hex(index + 1);
    nodes.push({
      id,
      hostname: `vpn${Math.floor(next() * 900_000 + 100_000)}`,
      ip: `${Math.floor(next() * 200 + 20)}.${Math.floor(next() * 255)}.${Math.floor(next() * 255)}.${Math.floor(next() * 254 + 1)}`,
      score: Math.floor(next() * 9_000_000 + 100_000),
      pingMs: next() > 0.08 ? Math.floor(next() * 280 + 8) : null,
      speedBps: Math.floor(next() * 58_000_000 + 900_000),
      countryLong,
      countryShort,
      sessions: Math.floor(next() * 60),
      uptimeMs: Math.floor(next() * 40 * 86_400_000),
      totalUsers: Math.floor(next() * 4_000_000),
      totalTrafficBytes: Math.floor(next() * 900_000_000_000_000),
      logType: LOG_TYPES[Math.floor(next() * LOG_TYPES.length)] ?? "unknown",
      operator: OPERATORS[Math.floor(next() * OPERATORS.length)] ?? "",
      message: next() > 0.7 ? "Public VPN Relay Server by Daiyuu Nobori" : "",
      tcpPort: next() > 0.05 ? [443, 995, 1194, 8888][Math.floor(next() * 4)] ?? 443 : null,
      availability,
      ...buildTest(id, availability === "available", next)
    });
  }
  return nodes;
}

function buildTest(
  nodeId: string,
  eligible: boolean,
  next: () => number
): { latestTest?: TestRecord } {
  if (!eligible || next() > 0.62) return {};
  const testedAt = new Date(Date.now() - Math.floor(next() * 6 * 3_600_000)).toISOString();
  if (next() > 0.88) {
    return {
      latestTest: {
        nodeId,
        durationMs: Math.floor(next() * 9_000 + 800),
        testedAt,
        error: "IPPure 请求超时"
      }
    };
  }
  return {
    latestTest: {
      nodeId,
      durationMs: Math.floor(next() * 3_000 + 400),
      testedAt,
      result: {
        fraudScore: Math.floor(next() * 100),
        isResidential: next() > 0.55,
        isBroadcast: next() > 0.7,
        exitIp: `${Math.floor(next() * 200 + 20)}.${Math.floor(next() * 255)}.${Math.floor(next() * 255)}.${Math.floor(next() * 254 + 1)}`
      }
    }
  };
}
