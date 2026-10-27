import { ChoiceChip, choicesOf, type Mode } from "./kit";

export function IridiumChips({ params, choices, onParams }: Mode<"iridium">) {
  return (
    <ChoiceChip
      label="Span"
      title="Iridium span"
      value={params.settings.span}
      options={choicesOf(choices, params.settings, "span")}
      onChange={(span) => onParams({ type: "iridium", settings: { ...params.settings, span } })}
    />
  );
}
