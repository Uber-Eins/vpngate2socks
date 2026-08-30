import { zhCN as t } from "../i18n";
import { useTheme, type ThemePreference } from "../state/useTheme";
import { Button } from "../ui/Button";
import { MonitorIcon, MoonIcon, SunIcon } from "../ui/Icon";

const ORDER: ThemePreference[] = ["system", "light", "dark"];
const LABELS: Record<ThemePreference, string> = {
  system: t.themeSystem,
  light: t.themeLight,
  dark: t.themeDark
};

export function ThemeToggle() {
  const [preference, setPreference] = useTheme();
  const next = ORDER[(ORDER.indexOf(preference) + 1) % ORDER.length] ?? "system";

  return (
    <Button
      variant="ghost"
      size="sm"
      title={`${t.theme}：${LABELS[preference]}`}
      aria-label={`${t.theme}：${LABELS[preference]}`}
      onClick={() => setPreference(next)}
    >
      {preference === "system" ? <MonitorIcon size={14} /> : null}
      {preference === "light" ? <SunIcon size={14} /> : null}
      {preference === "dark" ? <MoonIcon size={14} /> : null}
      <span className="rail__session-name">{LABELS[preference]}</span>
    </Button>
  );
}
