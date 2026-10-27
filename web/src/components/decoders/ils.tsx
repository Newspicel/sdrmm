import { ChoiceChip, choicesOf, limitOf, type Mode, NumberChip } from "./kit";

export function IlsChips({ params, choices, limits, onParams }: Mode<"ils">) {
  const settings = params.settings;
  return (
    <>
      <ChoiceChip
        label="Component"
        title="ILS component"
        value={settings.component}
        options={choicesOf(choices, params.settings, "component")}
        onChange={(component) => onParams({ type: "ils", settings: { ...settings, component } })}
      />
      <NumberChip
        label="Every"
        title="ILS report interval"
        value={settings.report_ms}
        {...limitOf(limits, "report_ms")}
        unit="ms"
        onCommit={(report_ms) => onParams({ type: "ils", settings: { ...settings, report_ms } })}
      />
    </>
  );
}
