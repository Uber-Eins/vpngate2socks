import { useSyncExternalStore } from "react";

export type ThemePreference = "system" | "light" | "dark";

const STORAGE_KEY = "v2s.theme";

/*
 * The preference lives in a module-level store rather than component state so the
 * toggle in the rail and the bootstrap in <App> always agree on one value, and so
 * the resolved theme is on <html> before the first screen paints.
 */
let preference: ThemePreference = readStoredPreference();
const listeners = new Set<() => void>();

const media = typeof window.matchMedia === "function"
  ? window.matchMedia("(prefers-color-scheme: light)")
  : undefined;

function apply(): void {
  const resolved = preference === "system"
    ? (media?.matches === true ? "light" : "dark")
    : preference;
  document.documentElement.dataset["theme"] = resolved;
}

media?.addEventListener("change", apply);
apply();

export function setThemePreference(value: ThemePreference): void {
  preference = value;
  apply();
  try {
    window.localStorage.setItem(STORAGE_KEY, value);
  } catch {
    // Private-mode storage failures must not break the toggle.
  }
  for (const listener of listeners) listener();
}

export function useTheme(): [ThemePreference, (value: ThemePreference) => void] {
  const value = useSyncExternalStore(subscribe, () => preference, () => preference);
  return [value, setThemePreference];
}

function subscribe(listener: () => void): () => void {
  listeners.add(listener);
  return () => listeners.delete(listener);
}

function readStoredPreference(): ThemePreference {
  try {
    const stored = window.localStorage.getItem(STORAGE_KEY);
    if (stored === "light" || stored === "dark" || stored === "system") return stored;
  } catch {
    // Ignore and fall back to the system preference.
  }
  return "system";
}
