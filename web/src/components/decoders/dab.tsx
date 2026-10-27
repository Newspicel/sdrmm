import { type BroadcastStatus, ChoiceChip, choicesOf, type Mode, ServiceChip } from "./kit";

export function DabChips({
  params,
  choices,
  broadcast,
  onParams,
}: Mode<"dab"> & { broadcast?: BroadcastStatus }) {
  const settings = params.settings;
  const mode = settings.mode;
  const transmissionMode = settings.transmission_mode;
  const detected = broadcast?.transmission_mode;
  const transmissionModes =
    detected == null
      ? choicesOf(choices, settings, "transmission_mode")
      : choicesOf(choices, settings, "transmission_mode").map((option) =>
          option.value === "auto" ? { ...option, label: `Auto ${detected.toUpperCase()}` } : option,
        );
  return (
    <>
      <ChoiceChip
        label="Type"
        title="DAB generation"
        value={mode}
        options={choicesOf(choices, params.settings, "mode")}
        quiet={mode === "auto"}
        onChange={(next) => onParams({ type: "dab", settings: { ...settings, mode: next } })}
      />
      <ChoiceChip
        label="Mode"
        title="DAB transmission mode"
        value={transmissionMode}
        options={transmissionModes}
        quiet={transmissionMode === "auto"}
        onChange={(transmission_mode) =>
          onParams({ type: "dab", settings: { ...settings, transmission_mode } })
        }
      />
      <ServiceChip
        status={broadcast}
        value={settings.service_id ?? null}
        max={0xffffffff}
        onChange={(service_id) => onParams({ type: "dab", settings: { ...settings, service_id } })}
      />
    </>
  );
}
