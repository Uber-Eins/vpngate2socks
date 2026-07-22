import { useEffect, useState, type FormEvent } from "react";

import { zhCN as t } from "../i18n";
import type { AutoConnectConfig, AutoConnectSettings } from "../types";

const DEFAULT_CONFIG: AutoConnectConfig = {
  enabled: false,
  ipType: "any",
  residential: "any"
};

export function AutoConnectPanel(props: {
  settings: AutoConnectSettings | undefined;
  busy: string | undefined;
  onSave: (config: AutoConnectConfig) => void;
}) {
  const [draft, setDraft] = useState(DEFAULT_CONFIG);
  const saved = props.settings?.config;

  useEffect(() => {
    if (saved !== undefined) setDraft(saved);
  }, [saved?.enabled, saved?.ipType, saved?.region, saved?.residential]);

  const changed = saved !== undefined && !sameConfig(saved, draft);
  const selectedRegionAvailable = draft.region === undefined
    || props.settings?.regions.some(({ code }) => code === draft.region) === true;
  const submit = (event: FormEvent) => {
    event.preventDefault();
    props.onSave(draft);
  };

  return (
    <section className="auto-connect" aria-labelledby="auto-connect-title">
      <div className="auto-connect__heading">
        <div>
          <p className="section-kicker">ROUTE POLICY</p>
          <h2 id="auto-connect-title">{t.autoConnect}</h2>
        </div>
        <span className={draft.enabled ? "policy-state policy-state--enabled" : "policy-state"}>
          {draft.enabled ? t.enabled : t.disabled}
        </span>
      </div>

      <form className="auto-connect__form" onSubmit={submit}>
        <label className="toggle-field">
          <input
            type="checkbox"
            checked={draft.enabled}
            disabled={props.settings === undefined || props.busy !== undefined}
            onChange={(event) => setDraft((current) => ({
              ...current,
              enabled: event.target.checked
            }))}
          />
          <span className="toggle-field__control" aria-hidden="true" />
          <span>
            <strong>{t.autoConnectToggle}</strong>
            <small>{t.autoConnectHint}</small>
          </span>
        </label>

        <div className="auto-connect__filters">
          <label className="select-field">
            <span>{t.regionFilter}</span>
            <select
              value={draft.region ?? ""}
              disabled={props.settings === undefined || props.busy !== undefined}
              onChange={(event) => setDraft((current) => withRegion(
                current,
                event.target.value
              ))}
            >
              <option value="">{t.anyRegion}</option>
              {!selectedRegionAvailable && draft.region !== undefined && (
                <option value={draft.region}>{draft.region}</option>
              )}
              {props.settings?.regions.map((region) => (
                <option key={region.code} value={region.code}>
                  {region.name} · {region.code}
                </option>
              ))}
            </select>
          </label>

          <label className="select-field">
            <span>{t.ipTypeFilter}</span>
            <select
              value={draft.ipType}
              disabled={props.settings === undefined || props.busy !== undefined}
              onChange={(event) => setDraft((current) => ({
                ...current,
                ipType: event.target.value as AutoConnectConfig["ipType"]
              }))}
            >
              <option value="any">{t.anyIpType}</option>
              <option value="native">{t.nativeIp}</option>
              <option value="broadcast">{t.broadcastIp}</option>
            </select>
          </label>

          <label className="select-field">
            <span>{t.residentialFilter}</span>
            <select
              value={draft.residential}
              disabled={props.settings === undefined || props.busy !== undefined}
              onChange={(event) => setDraft((current) => ({
                ...current,
                residential: event.target.value as AutoConnectConfig["residential"]
              }))}
            >
              <option value="any">{t.anyResidential}</option>
              <option value="residential">{t.residentialOnly}</option>
              <option value="nonResidential">{t.nonResidentialOnly}</option>
            </select>
          </label>
        </div>

        <div className="auto-connect__strategy">
          <span>{t.selectionStrategy}</span>
          <strong>{t.highestBandwidth}</strong>
          <small>{t.reconnectHint}</small>
        </div>

        <button
          className="button button--primary"
          type="submit"
          disabled={!changed || props.busy !== undefined}
        >
          {props.busy === "autoConnect" ? t.saving : t.savePolicy}
        </button>
      </form>
    </section>
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
