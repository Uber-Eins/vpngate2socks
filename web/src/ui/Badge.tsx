import type { ReactNode } from "react";

import { cx } from "../utils/cx";

export type Tone = "neutral" | "accent" | "info" | "warning" | "danger";
export type DotTone = "idle" | "online" | "busy" | "danger";

export function Badge({ tone = "neutral", children }: { tone?: Tone; children: ReactNode }) {
  return <span className={`badge badge--${tone}`}>{children}</span>;
}

export function Dot({ tone }: { tone: DotTone }) {
  return <span className={cx("dot", tone !== "idle" && `dot--${tone}`)} aria-hidden="true" />;
}

/** Compact `label: value` capsule used across the top bar. */
export function StatusPill({ tone, dot, label, children, title }: {
  tone?: "default" | "danger";
  dot?: DotTone;
  label: string;
  children: ReactNode;
  title?: string;
}) {
  return (
    <span
      className={cx("status-pill", tone === "danger" && "status-pill--danger")}
      {...(title === undefined ? {} : { title })}
    >
      {dot !== undefined && <Dot tone={dot} />}
      {label}
      <strong>{children}</strong>
    </span>
  );
}
