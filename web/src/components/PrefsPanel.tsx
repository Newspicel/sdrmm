import { useState } from "react";
import { LANGUAGES } from "../lib/i18n/languages";
import { useT } from "../lib/i18n/translate";
import { setLocale, setTextSize, TEXT_SIZE, usePrefs } from "../lib/prefs";
import { setTheme, THEME_CYCLE, useTheme } from "../lib/theme";
import { Select } from "./Select";
import { SettingsPanel, SettingsSection } from "./SettingsPanel";
import { SliderField } from "./Slider";
import { THEME_NAMES } from "./ThemeControl";

const LANGUAGE_OPTIONS = LANGUAGES.map((language) => ({
  value: language.id,
  label: language.name,
}));

/** Settings of this browser only: nothing here reaches the server or the workspace. */
export function PrefsPanel() {
  const t = useT();
  const { choice } = useTheme();
  const { locale } = usePrefs();
  const themes = THEME_CYCLE.map((theme) => ({ value: theme, label: t(THEME_NAMES[theme]) }));
  return (
    <SettingsPanel>
      <SettingsSection name={t("prefs.theme")}>
        <Select label={t("prefs.theme")} value={choice} options={themes} onChange={setTheme} />
      </SettingsSection>
      <SettingsSection name={t("prefs.textSize")}>
        <TextSizeSlider label={t("prefs.textSize")} />
      </SettingsSection>
      <SettingsSection name={t("prefs.language")}>
        <Select
          label={t("prefs.language")}
          value={locale}
          options={LANGUAGE_OPTIONS}
          onChange={setLocale}
        />
      </SettingsSection>
    </SettingsPanel>
  );
}

/// The size applies on release: resizing the page under a dragged thumb would move its track.
function TextSizeSlider({ label }: { label: string }) {
  const { textSize } = usePrefs();
  const [draft, setDraft] = useState<number | null>(null);
  const shown = draft ?? textSize;
  return (
    <span className="flex max-w-80">
      <SliderField
        label={label}
        min={TEXT_SIZE.min}
        max={TEXT_SIZE.max}
        step={TEXT_SIZE.step}
        value={shown}
        onChange={setDraft}
        onCommit={(percent) => {
          setDraft(null);
          setTextSize(percent);
        }}
        readout={`${shown} %`}
      />
    </span>
  );
}
