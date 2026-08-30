import { zhCN as t } from "../i18n";
import type { AutoConnectSettings } from "../types";
import { Badge } from "../ui/Badge";
import { Button } from "../ui/Button";
import { Card } from "../ui/Card";
import { Skeleton } from "../ui/Feedback";
import { ipTypeLabel, regionLabel, residentialLabel } from "../policy/labels";

export function PolicyRecapCard({ settings, onEdit }: {
  settings: AutoConnectSettings | undefined;
  onEdit: () => void;
}) {
  const config = settings?.config;

  return (
    <Card
      title={t.policySummary}
      actions={
        config === undefined ? null : (
          <Badge tone={config.enabled ? "accent" : "neutral"}>
            {config.enabled ? t.policyEnabled : t.policyDisabled}
          </Badge>
        )
      }
      footer={<Button onClick={onEdit}>{t.editPolicy}</Button>}
    >
      {config === undefined ? (
        <div className="skeleton-stack">
          <Skeleton />
          <Skeleton width="half" />
          <Skeleton width="short" />
        </div>
      ) : (
        <div className="recap">
          <Row label={t.region} value={regionLabel(config.region, settings?.regions ?? [])} />
          <Row label={t.ipTypeFilter} value={ipTypeLabel(config.ipType)} />
          <Row label={t.residentialFilter} value={residentialLabel(config.residential)} />
          <Row label={t.strategy} value={t.strategyValue} />
        </div>
      )}
    </Card>
  );
}

function Row({ label, value }: { label: string; value: string }) {
  return (
    <div className="recap__row">
      <span className="recap__label">{label}</span>
      <span className="recap__value" title={value}>{value}</span>
    </div>
  );
}
