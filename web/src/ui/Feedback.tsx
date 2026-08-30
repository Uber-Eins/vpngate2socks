import type { ReactNode } from "react";

import { cx } from "../utils/cx";
import { AlertIcon, InfoIcon } from "./Icon";

export function Spinner({ large = false }: { large?: boolean }) {
  return <span className={cx("spinner", large && "spinner--lg")} aria-hidden="true" />;
}

export type NoticeTone = "neutral" | "info" | "warning" | "danger";

export function Notice({ tone, children }: { tone: NoticeTone; children: ReactNode }) {
  return (
    <div className={`notice notice--${tone}`} role={tone === "danger" ? "alert" : "status"}>
      <span className="notice__icon">
        {tone === "danger" || tone === "warning" ? <AlertIcon /> : <InfoIcon />}
      </span>
      <span>{children}</span>
    </div>
  );
}

export function EmptyState({ icon, title, description }: {
  icon: ReactNode;
  title: string;
  description: string;
}) {
  return (
    <div className="empty">
      <span className="empty__icon">{icon}</span>
      <strong>{title}</strong>
      <p>{description}</p>
    </div>
  );
}

export function Skeleton({ width = "wide" }: { width?: "wide" | "half" | "short" }) {
  return <span className={`skeleton skeleton--${width}`} aria-hidden="true" />;
}
