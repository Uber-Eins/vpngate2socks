import type { ReactNode } from "react";

import { cx } from "../utils/cx";

export function Card({ title, description, actions, footer, flush, className, children }: {
  title?: string;
  description?: string;
  actions?: ReactNode;
  footer?: ReactNode;
  flush?: boolean;
  className?: string;
  children: ReactNode;
}) {
  return (
    <section className={cx("card", flush === true && "card--flush", className)}>
      {title !== undefined && (
        <header className="card__header">
          <div className="card__title">
            <h2>{title}</h2>
            {description !== undefined && <p>{description}</p>}
          </div>
          {actions}
        </header>
      )}
      <div className="card__body">{children}</div>
      {footer !== undefined && <footer className="card__footer">{footer}</footer>}
    </section>
  );
}

export function Fact({ label, children }: { label: string; children: ReactNode }) {
  return (
    <div className="fact">
      <span className="fact__label">{label}</span>
      <span className="fact__value">{children}</span>
    </div>
  );
}
