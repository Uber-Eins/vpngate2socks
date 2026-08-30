/** VPN Gate reports bandwidth in bytes per second; the UI shows bits per second. */
export function formatBits(bytesPerSecond: number): string {
  const bits = bytesPerSecond * 8;
  if (bits >= 1_000_000_000) return `${(bits / 1_000_000_000).toFixed(1)} Gbps`;
  if (bits >= 1_000_000) return `${(bits / 1_000_000).toFixed(1)} Mbps`;
  return `${Math.round(bits / 1_000)} Kbps`;
}

export function formatBytes(bytes: number): string {
  const units = ["B", "KiB", "MiB", "GiB", "TiB", "PiB"];
  let value = bytes;
  let unit = 0;
  while (value >= 1024 && unit < units.length - 1) {
    value /= 1024;
    unit += 1;
  }
  return `${unit === 0 ? value : value.toFixed(1)} ${units[unit] ?? "B"}`;
}

/** Compact duration such as `3 天 4 小时` or `12 分 09 秒`, for uptime and elapsed time. */
export function formatDuration(milliseconds: number): string {
  const total = Math.max(0, Math.round(milliseconds / 1000));
  const days = Math.floor(total / 86_400);
  const hours = Math.floor((total % 86_400) / 3600);
  const minutes = Math.floor((total % 3600) / 60);
  const seconds = total % 60;
  if (days > 0) return `${days} 天 ${hours} 小时`;
  if (hours > 0) return `${hours} 小时 ${minutes} 分`;
  if (minutes > 0) return `${minutes} 分 ${pad(seconds)} 秒`;
  return `${seconds} 秒`;
}

/** Time elapsed since an ISO timestamp, relative to `now` so callers control ticking. */
export function formatElapsed(since: string, now: number): string {
  const started = Date.parse(since);
  return Number.isNaN(started) ? "—" : formatDuration(now - started);
}

export function formatDateTime(value: string): string {
  const parsed = new Date(value);
  return Number.isNaN(parsed.getTime())
    ? "—"
    : parsed.toLocaleString("zh-CN", { hour12: false });
}

export function formatRelative(value: string, now: number): string {
  const parsed = Date.parse(value);
  if (Number.isNaN(parsed)) return "—";
  const seconds = Math.round((now - parsed) / 1000);
  if (seconds < 60) return "刚刚";
  return `${formatDuration(seconds * 1000)}前`;
}

export function formatNumber(value: number): string {
  return value.toLocaleString("zh-CN");
}

function pad(value: number): string {
  return value.toString().padStart(2, "0");
}
