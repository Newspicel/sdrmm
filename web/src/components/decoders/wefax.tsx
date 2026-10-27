import { ChoiceChip, choicesOf, type Mode, ToggleChip } from "./kit";

export function WefaxChips({ params, choices, onParams }: Mode<"wefax">) {
  const settings = params.settings;
  const set = (next: typeof settings) => onParams({ type: "wefax", settings: next });
  return (
    <>
      <ChoiceChip
        label="IOC"
        title="Index of cooperation"
        value={settings.ioc}
        options={choicesOf(choices, params.settings, "ioc")}
        onChange={(ioc) => set({ ...settings, ioc })}
      />
      <ChoiceChip
        label="LPM"
        title="Lines per minute"
        value={settings.lpm}
        options={choicesOf(choices, params.settings, "lpm")}
        onChange={(lpm) => set({ ...settings, lpm })}
      />
      <ToggleChip
        label="Partial"
        title="Keep unfinished charts"
        on={settings.keep_partial}
        onChange={(keep_partial) => set({ ...settings, keep_partial })}
      />
    </>
  );
}
