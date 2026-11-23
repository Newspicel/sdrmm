import { type LucideIcon, Monitor, Moon, Sun } from "lucide-react";
import { type MessageKey, useT } from "../lib/i18n/translate";
import { nextTheme, setTheme, type ThemeChoice, useTheme } from "../lib/theme";
import { Button } from "./BaseControls";
import { ICON_BTN } from "./controls";
import { Icon } from "./Icon";

const THEME_ICONS: Record<ThemeChoice, LucideIcon> = {
  system: Monitor,
  dark: Moon,
  light: Sun,
};

export const THEME_NAMES: Record<ThemeChoice, MessageKey> = {
  system: "theme.system",
  dark: "theme.dark",
  light: "theme.light",
};

export function ThemeControl() {
  const t = useT();
  const { choice } = useTheme();
  const next = nextTheme(choice);
  const names = { current: t(THEME_NAMES[choice]), next: t(THEME_NAMES[next]) };
  return (
    <Button
      type="button"
      className={ICON_BTN}
      aria-label={t("theme.switch", names)}
      title={t("theme.title", names)}
      onClick={() => setTheme(next)}
    >
      <Icon glyph={THEME_ICONS[choice]} size={16} />
    </Button>
  );
}
