import {
  type BroadcastStatus,
  ChoiceChip,
  choicesOf,
  type Mode,
  OptionalNumberChip,
  ServiceChip,
  ToggleChip,
} from "./kit";

export function DvbtChips({
  params,
  choices,
  broadcast,
  onParams,
}: Mode<"dvbt"> & { broadcast?: BroadcastStatus }) {
  const settings = params.settings;
  const set = (next: typeof settings) => onParams({ type: "dvbt", settings: next });
  return (
    <>
      <ChoiceChip
        label="Standard"
        title="Terrestrial standard"
        value={settings.standard}
        options={choicesOf(choices, params.settings, "standard")}
        onChange={(standard) => set({ ...settings, standard })}
      />
      <ChoiceChip
        label="BW"
        title="DVB-T bandwidth"
        value={settings.bandwidth}
        options={choicesOf(choices, params.settings, "bandwidth")}
        onChange={(bandwidth) => set({ ...settings, bandwidth })}
      />
      {settings.standard === "dvb_t2" ? (
        <OptionalNumberChip
          label="PLP"
          title="PLP ID, empty for automatic selection"
          placeholder="auto"
          value={settings.plp ?? null}
          min={0}
          max={255}
          step={1}
          onCommit={(plp) => set({ ...settings, plp })}
        />
      ) : (
        <ToggleChip
          label="Low priority"
          title="Decode the low priority transport stream of a hierarchical DVB-T multiplex"
          on={settings.low_priority}
          onChange={(low_priority) => set({ ...settings, low_priority })}
        />
      )}
      <ServiceChip
        status={broadcast}
        value={settings.program ?? null}
        max={65535}
        onChange={(program) => set({ ...settings, program })}
      />
    </>
  );
}
