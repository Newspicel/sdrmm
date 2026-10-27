import { ChoiceChip, choicesOf, type Mode } from "./kit";

export function AeroChips({ params, choices, onParams }: Mode<"inmarsat_aero">) {
  return (
    <ChoiceChip
      label="Channel"
      title="Aero channel"
      value={params.settings.channel}
      options={choicesOf(choices, params.settings, "channel")}
      onChange={(channel) =>
        onParams({ type: "inmarsat_aero", settings: { ...params.settings, channel } })
      }
    />
  );
}
