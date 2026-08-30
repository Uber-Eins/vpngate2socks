import { zhCN as t } from "../i18n";
import type { IpTypeFilter, RegionOption, ResidentialFilter } from "../types";

export function regionLabel(code: string | undefined, regions: RegionOption[]): string {
  if (code === undefined || code === "") return t.anyRegion;
  const match = regions.find((region) => region.code === code);
  return match === undefined ? code : `${match.name} · ${match.code}`;
}

export function ipTypeLabel(value: IpTypeFilter): string {
  switch (value) {
    case "native":
      return t.nativeIp;
    case "broadcast":
      return t.broadcastIp;
    default:
      return t.anyIpType;
  }
}

export function residentialLabel(value: ResidentialFilter): string {
  switch (value) {
    case "residential":
      return t.residentialOnly;
    case "nonResidential":
      return t.nonResidentialOnly;
    default:
      return t.anyResidential;
  }
}

export function regionOptions(regions: RegionOption[]): { value: string; label: string }[] {
  return [
    { value: "", label: t.anyRegion },
    ...regions.map((region) => ({ value: region.code, label: `${region.name} · ${region.code}` }))
  ];
}

export const IP_TYPE_OPTIONS = [
  { value: "any", label: t.anyIpType },
  { value: "native", label: t.nativeIp },
  { value: "broadcast", label: t.broadcastIp }
];

export const RESIDENTIAL_OPTIONS = [
  { value: "any", label: t.anyResidential },
  { value: "residential", label: t.residentialOnly },
  { value: "nonResidential", label: t.nonResidentialOnly }
];
