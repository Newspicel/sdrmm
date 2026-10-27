import {
  BandwidthChip,
  type BroadcastStatus,
  ChoiceChip,
  choicesOf,
  type Mode,
  ServiceChip,
} from "./kit";

export function DrmChips({
  params,
  choices,
  broadcast,
  onParams,
}: Mode<"drm"> & { broadcast?: BroadcastStatus }) {
  const settings = params.settings;
  const mode = settings.mode;
  return (
    <>
      <ChoiceChip
        label="Mode"
        title="DRM mode"
        value={mode}
        options={choicesOf(choices, params.settings, "mode")}
        quiet={mode === "auto"}
        onChange={(next) =>
          onParams({
            type: "drm",
            settings: {
              ...settings,
              mode: next,
              bandwidth_hz:
                next === "drm30" ? (mode === "drm30" ? settings.bandwidth_hz : 10_000) : 100_000,
            },
          })
        }
      />
      <BandwidthChip
        valueHz={settings.bandwidth_hz}
        optionsHz={mode === "drm30" ? [4_500, 5_000, 9_000, 10_000, 18_000, 20_000] : [100_000]}
        onCommit={(bandwidth_hz) =>
          onParams({ type: "drm", settings: { ...settings, bandwidth_hz } })
        }
      />
      <ServiceChip
        status={broadcast}
        value={settings.service ?? null}
        max={3}
        onChange={(service) => onParams({ type: "drm", settings: { ...settings, service } })}
      />
    </>
  );
}
