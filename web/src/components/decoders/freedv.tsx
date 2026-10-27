import { ChoiceChip, choicesOf, type Mode } from "./kit";

export function FreedvChips({ params, choices, onParams }: Mode<"freedv">) {
  return (
    <ChoiceChip
      label="Side"
      title="FreeDV sideband"
      value={params.settings.sideband}
      options={choicesOf(choices, params.settings, "sideband")}
      onChange={(sideband) =>
        onParams({ type: "freedv", settings: { ...params.settings, sideband } })
      }
    />
  );
}
