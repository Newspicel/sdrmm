import { ChoiceChip, choicesOf, type Mode } from "./kit";

export function NxdnChips({ params, choices, onParams }: Mode<"nxdn">) {
  return (
    <ChoiceChip
      label="Width"
      title="Width"
      value={params.settings.bandwidth}
      options={choicesOf(choices, params.settings, "bandwidth")}
      onChange={(bandwidth) =>
        onParams({ type: "nxdn", settings: { ...params.settings, bandwidth } })
      }
    />
  );
}
