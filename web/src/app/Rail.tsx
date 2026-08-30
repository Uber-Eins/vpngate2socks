import { zhCN as t } from "../i18n";
import type { View } from "../state/useView";
import { Button } from "../ui/Button";
import {
  ActivityIcon,
  LockIcon,
  LogOutIcon,
  ServerIcon,
  ShieldIcon,
  SlidersIcon
} from "../ui/Icon";
import { cx } from "../utils/cx";
import { ThemeToggle } from "./ThemeToggle";

const ITEMS = [
  { view: "overview", label: t.navOverview, Icon: ActivityIcon },
  { view: "nodes", label: t.navNodes, Icon: ServerIcon },
  { view: "policy", label: t.navPolicy, Icon: SlidersIcon },
  { view: "security", label: t.navSecurity, Icon: LockIcon }
] as const satisfies readonly { view: View; label: string; Icon: typeof ActivityIcon }[];

export function Rail({ view, nodeCount, canLogout, busy, onView, onLogout }: {
  view: View;
  nodeCount: number | undefined;
  canLogout: boolean;
  busy: boolean;
  onView: (view: View) => void;
  onLogout: () => void;
}) {
  return (
    <nav className="rail" aria-label={t.navSection}>
      <div className="rail__brand">
        <span className="brand-mark"><ShieldIcon size={17} /></span>
        <span className="rail__brand-text">
          <strong>{t.brand}</strong>
          <span>{t.brandTagline}</span>
        </span>
      </div>

      <div className="rail__nav">
        <p className="rail__group-label">{t.navSection}</p>
        {ITEMS.map(({ view: item, label, Icon }) => (
          <button
            key={item}
            type="button"
            className={cx("nav-item", item === view && "nav-item--active")}
            aria-current={item === view ? "page" : undefined}
            onClick={() => onView(item)}
          >
            <span className="nav-item__icon"><Icon /></span>
            <span>{label}</span>
            {item === "nodes" && nodeCount !== undefined && (
              <span className="nav-item__count">{nodeCount.toLocaleString("zh-CN")}</span>
            )}
          </button>
        ))}
      </div>

      <div className="rail__footer">
        <ThemeToggle />
        {canLogout && (
          <div className="rail__session">
            <Button variant="ghost" size="sm" disabled={busy} onClick={onLogout}>
              <LogOutIcon size={14} />
              <span className="rail__session-name">{t.logout}</span>
            </Button>
          </div>
        )}
      </div>
    </nav>
  );
}
