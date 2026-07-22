import type { FormEvent } from "react";

import { zhCN as t } from "../i18n";

export function NodeToolbar(props: {
  draftSearch: string;
  sort: string;
  order: string;
  busy: string | undefined;
  canDisconnect: boolean;
  canLogout: boolean;
  onDraftSearch: (value: string) => void;
  onSearch: () => void;
  onSort: (value: string) => void;
  onOrder: (value: string) => void;
  onRefresh: () => void;
  onDisconnect: () => void;
  onLogout: () => void;
}) {
  const submit = (event: FormEvent) => {
    event.preventDefault();
    props.onSearch();
  };

  return (
    <div className="node-toolbar" aria-label="节点工具栏">
      <form className="search-field" onSubmit={submit} role="search">
        <span className="search-field__icon" aria-hidden="true" />
        <input
          type="search"
          aria-label={t.searchPlaceholder}
          value={props.draftSearch}
          onChange={(event) => props.onDraftSearch(event.target.value)}
          placeholder={t.searchPlaceholder}
        />
        <button className="button button--primary" type="submit" disabled={props.busy !== undefined}>
          {t.search}
        </button>
      </form>
      <div className="toolbar-fields">
        <label className="select-field">
          <span>{t.sortBy}</span>
          <select value={props.sort} onChange={(event) => props.onSort(event.target.value)}>
            <option value="score">{t.score}</option>
            <option value="ping">{t.ping}</option>
            <option value="speed">{t.bandwidth}</option>
            <option value="sessions">{t.sessions}</option>
          </select>
        </label>
        <label className="select-field select-field--compact">
          <span>{t.sortOrder}</span>
          <select value={props.order} onChange={(event) => props.onOrder(event.target.value)}>
            <option value="desc">{t.descending}</option>
            <option value="asc">{t.ascending}</option>
          </select>
        </label>
      </div>
      <div className="toolbar-actions">
        <button
          className="button button--quiet"
          type="button"
          disabled={props.busy !== undefined}
          onClick={props.onRefresh}
        >
          <span className="refresh-icon" aria-hidden="true" />
          {props.busy === "refresh" ? t.refreshing : t.refresh}
        </button>
        <button
          className="button button--danger"
          type="button"
          disabled={!props.canDisconnect || props.busy !== undefined}
          onClick={props.onDisconnect}
        >
          {t.disconnect}
        </button>
        {props.canLogout && (
          <button
            className="button button--quiet"
            type="button"
            disabled={props.busy !== undefined}
            onClick={props.onLogout}
          >
            {t.logout}
          </button>
        )}
      </div>
    </div>
  );
}
