import type { ReactNode } from "react";

import { cx } from "../utils/cx";

export function StatTile({ icon, label, value, unit, meta, tone }: {
  icon: ReactNode;
  label: string;
  value: ReactNode;
  unit?: string;
  meta?: string;
  tone?: "default" | "accent" | "danger";
}) {
  return (
    <article className={cx("tile", tone !== undefined && tone !== "default" && `tile--${tone}`)}>
      <span className="tile__label">
        {icon}
        {label}
      </span>
      <span className="tile__value">
        {value}
        {unit !== undefined && <small>{unit}</small>}
      </span>
      {meta !== undefined && <span className="tile__meta" title={meta}>{meta}</span>}
    </article>
  );
}
