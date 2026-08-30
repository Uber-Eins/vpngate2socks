import { useEffect, useState, type FormEvent } from "react";

import { zhCN as t } from "../i18n";
import { IP_TYPE_OPTIONS, RESIDENTIAL_OPTIONS, regionOptions } from "../policy/labels";
import type { AutoConnectConfig, AutoConnectSettings, IpTypeFilter, ResidentialFilter } from "../types";
import { Badge } from "../ui/Badge";
import { Button } from "../ui/Button";
import { Card } from "../ui/Card";
import { SelectField, Switch } from "../ui/Field";
import { SlidersIcon } from "../ui/Icon";

const DEFAULT_CONFIG: AutoConnectConfig = {
  enabled: false,
  ipType: "any",
  residential: "any"
};

export function PolicyView({ settings, busy, onSave }: {
  settings: AutoConnectSettings | undefined;
  busy: string | undefined;
  onSave: (config: AutoConnectConfig) => void;
}) {
  const [draft, setDraft] = useState(DEFAULT_CONFIG);
  const saved = settings?.config;

  useEffect(() => {
    if (saved !== undefined) setDraft(saved);
  }, [saved?.enabled, saved?.ipType, saved?.region, saved?.residential]);

  const changed = saved !== undefined && !sameConfig(saved, draft);
  const disabled = settings === undefined || busy !== undefined;
  const selectedRegionMissing =
    draft.region !== undefined &&
    settings?.regions.some(({ code }) => code === draft.region) !== true;
  const submit = (event: FormEvent) => {
    event.preventDefault();
    onSave(draft);
  };

  return (
    <>
      <div className="view-heading">
        <div>
          <h1>{t.policyTitle}</h1>
          <p>{t.policySubtitle}</p>
        </div>
        <Badge tone={draft.enabled ? "accent" : "neutral"}>
          {draft.enabled ? t.policyEnabled : t.policyDisabled}
        </Badge>
      </div>

      <form onSubmit={submit}>
        <Card
          footer={
            <>
              <span className="muted">{changed ? t.unsavedChanges : ""}</span>
              <span className="row-actions">
                {changed && (
                  <Button
                    disabled={disabled}
                    onClick={() => setDraft(saved ?? DEFAULT_CONFIG)}
                  >
                    {t.discardChanges}
                  </Button>
                )}
                <Button
                  type="submit"
                  variant="primary"
                  disabled={!changed || busy !== undefined}
                  busy={busy === "autoConnect"}
                >
                  {busy === "autoConnect" ? t.saving : t.savePolicy}
                </Button>
              </span>
            </>
          }
        >
          <div className="policy-form">
            <Switch
              checked={draft.enabled}
              disabled={disabled}
              title={t.enablePolicy}
              description={t.enablePolicyHint}
              onChange={(enabled) => setDraft((current) => ({ ...current, enabled }))}
            />

            <div className="policy-form__filters">
              <SelectField
                label={t.regionFilter}
                value={draft.region ?? ""}
                disabled={disabled}
                options={[
                  ...regionOptions(settings?.regions ?? []),
                  ...(selectedRegionMissing && draft.region !== undefined
                    ? [{ value: draft.region, label: draft.region }]
                    : [])
                ]}
                onChange={(region) => setDraft((current) => withRegion(current, region))}
              />
              <SelectField
                label={t.ipTypeFilter}
                value={draft.ipType}
                disabled={disabled}
                options={IP_TYPE_OPTIONS}
                onChange={(value) =>
                  setDraft((current) => ({ ...current, ipType: value as IpTypeFilter }))
                }
              />
              <SelectField
                label={t.residentialFilter}
                value={draft.residential}
                disabled={disabled}
                options={RESIDENTIAL_OPTIONS}
                onChange={(value) =>
                  setDraft((current) => ({ ...current, residential: value as ResidentialFilter }))
                }
              />
            </div>

            <div className="policy-form__strategy">
              <SlidersIcon className="policy-form__strategy-icon" />
              <div>
                <strong>{t.strategy}：{t.strategyValue}</strong>
                <p>{t.strategyHint}</p>
              </div>
            </div>
          </div>
        </Card>
      </form>
    </>
  );
}

function sameConfig(left: AutoConnectConfig, right: AutoConnectConfig): boolean {
  return left.enabled === right.enabled
    && left.region === right.region
    && left.ipType === right.ipType
    && left.residential === right.residential;
}

function withRegion(config: AutoConnectConfig, region: string): AutoConnectConfig {
  const next = { ...config };
  if (region === "") {
    delete next.region;
  } else {
    next.region = region;
  }
  return next;
}
