import { ChoiceChip, choicesOf, InvertChip, type Mode } from "./kit";

export function RadioClockChips({ params, choices, onParams }: Mode<"radio_clock">) {
  const settings = params.settings;
  return (
    <>
      <ChoiceChip
        label="Service"
        title="Radio clock service"
        value={settings.standard}
        options={choicesOf(choices, params.settings, "standard")}
        onChange={(standard) =>
          onParams({ type: "radio_clock", settings: { ...settings, standard } })
        }
      />
      <InvertChip
        on={settings.invert}
        onChange={(invert) => onParams({ type: "radio_clock", settings: { ...settings, invert } })}
      />
    </>
  );
}
