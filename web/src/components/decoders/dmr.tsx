import { ChoiceChip, choicesOf, type Mode, ToggleChip } from "./kit";

export function DmrChips({ params, choices, onParams }: Mode<"dmr">) {
  const settings = params.settings;
  return (
    <>
      <ChoiceChip
        label="Slot"
        title="Slot"
        value={settings.slots}
        options={choicesOf(choices, params.settings, "slots")}
        onChange={(slots) => onParams({ type: "dmr", settings: { ...settings, slots } })}
      />
      <ToggleChip
        label="Skip CRC"
        title="Ignore data CRC"
        on={settings.ignore_crc}
        onChange={(ignore_crc) => onParams({ type: "dmr", settings: { ...settings, ignore_crc } })}
      />
    </>
  );
}
