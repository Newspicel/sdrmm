import { ChoiceChip, choicesOf, type Mode } from "./kit";

export function SelcallChips({ params, choices, onParams }: Mode<"selcall">) {
  return (
    <ChoiceChip
      label="Plan"
      title="Selective calling tone plan"
      value={params.settings.system}
      options={choicesOf(choices, params.settings, "system")}
      onChange={(system) => onParams({ type: "selcall", settings: { ...params.settings, system } })}
    />
  );
}
