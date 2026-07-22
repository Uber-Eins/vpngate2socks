import { zhCN as t } from "../i18n";
import type { StatusSnapshot } from "../types";
import { StatusOverview } from "./StatusOverview";

export function DashboardHeader({ status }: { status: StatusSnapshot | undefined }) {
  return (
    <header className="dashboard-header">
      <div className="dashboard-header__brand">
        <div className="brand-mark" aria-hidden="true"><span /></div>
        <div>
          <p className="eyebrow">{t.eyebrow}</p>
          <span className="architecture-label">{t.architecture}</span>
        </div>
      </div>
      <div className="dashboard-header__copy">
        <h1>{t.title}</h1>
        <p>{t.subtitle}</p>
      </div>
      <StatusOverview status={status} />
    </header>
  );
}
