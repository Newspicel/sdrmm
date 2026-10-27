import { type Mode, ToggleChip } from "./kit";

export function IsmChips({ params, onParams }: Mode<"ism">) {
  return (
    <ToggleChip
      label="Unknown"
      title="Also log repeated codes no decoder knows"
      on={params.settings.unknown}
      onChange={(unknown) => onParams({ type: "ism", settings: { ...params.settings, unknown } })}
    />
  );
}
