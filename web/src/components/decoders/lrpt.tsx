import { ChoiceChip, choicesOf, type Mode } from "./kit";

export function LrptChips({ params, choices, onParams }: Mode<"lrpt">) {
  const settings = params.settings;
  return (
    <ChoiceChip
      label="Mode"
      title="Meteor-M downlink mode"
      value={settings.mode}
      options={choicesOf(choices, params.settings, "mode")}
      onChange={(mode) => onParams({ type: "lrpt", settings: { ...settings, mode } })}
    />
  );
}
