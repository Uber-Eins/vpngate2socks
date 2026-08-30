import type { FormEvent } from "react";

import { zhCN as t } from "../i18n";
import { IP_TYPE_OPTIONS, RESIDENTIAL_OPTIONS, regionOptions } from "../policy/labels";
import { DEFAULT_QUERY } from "../state/useConsole";
import type {
  AvailabilityFilter,
  IpTypeFilter,
  NodeQuery,
  RegionOption,
  ResidentialFilter
} from "../types";
import { Button } from "../ui/Button";
import { SelectField } from "../ui/Field";
import { CloseIcon, SearchIcon } from "../ui/Icon";

const AVAILABILITY_OPTIONS = [
  { value: "any", label: t.anyAvailability },
  { value: "available", label: t.connectableOnly }
];

export function NodeFilters({ query, draftSearch, regions, busy, onDraftSearch, onSearch, onFilter, onReset }: {
  query: NodeQuery;
  draftSearch: string;
  regions: RegionOption[];
  busy: boolean;
  onDraftSearch: (value: string) => void;
  onSearch: () => void;
  onFilter: <K extends keyof NodeQuery>(key: K, value: NodeQuery[K]) => void;
  onReset: () => void;
}) {
  const submit = (event: FormEvent) => {
    event.preventDefault();
    onSearch();
  };
  const filtered =
    query.search !== DEFAULT_QUERY.search ||
    query.region !== DEFAULT_QUERY.region ||
    query.ipType !== DEFAULT_QUERY.ipType ||
    query.residential !== DEFAULT_QUERY.residential ||
    query.availability !== DEFAULT_QUERY.availability;
  const classificationFiltered = query.ipType !== "any" || query.residential !== "any";

  return (
    <>
      <div className="filters">
        <form className="filters__group filters__group--grow" role="search" onSubmit={submit}>
          <div className="search">
            <SearchIcon className="search__icon" size={14} />
            <input
              className="input"
              type="search"
              aria-label={t.search}
              placeholder={t.searchPlaceholder}
              value={draftSearch}
              onChange={(event) => onDraftSearch(event.target.value)}
            />
            {draftSearch !== "" && (
              <Button
                className="search__clear"
                variant="ghost"
                size="sm"
                iconOnly
                aria-label={t.clearSearch}
                onClick={() => {
                  onDraftSearch("");
                  onFilter("search", "");
                }}
              >
                <CloseIcon size={13} />
              </Button>
            )}
          </div>
          <Button type="submit" disabled={busy}>{t.search}</Button>
        </form>

        {filtered && (
          <div className="filters__reset">
            <Button variant="ghost" onClick={onReset}>{t.resetFilters}</Button>
          </div>
        )}

        <div className="filters__group">
          <SelectField
            label={t.regionFilter}
            value={query.region}
            options={regionOptions(regions)}
            onChange={(value) => onFilter("region", value)}
          />
          <SelectField
            label={t.ipTypeFilter}
            value={query.ipType}
            options={IP_TYPE_OPTIONS}
            onChange={(value) => onFilter("ipType", value as IpTypeFilter)}
          />
          <SelectField
            label={t.residentialFilter}
            value={query.residential}
            options={RESIDENTIAL_OPTIONS}
            onChange={(value) => onFilter("residential", value as ResidentialFilter)}
          />
          <SelectField
            label={t.availabilityFilter}
            value={query.availability}
            options={AVAILABILITY_OPTIONS}
            onChange={(value) => onFilter("availability", value as AvailabilityFilter)}
          />
        </div>
      </div>

      {classificationFiltered && (
        <p className="active-filters muted">{t.filterNotice}</p>
      )}
    </>
  );
}
