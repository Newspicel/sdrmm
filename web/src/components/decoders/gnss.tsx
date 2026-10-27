import { HzChip, limitOf, type Mode, NumberChip } from "./kit";

export function GnssChips({ params, limits, onParams }: Mode<"gnss">) {
  const settings = params.settings;
  return (
    <>
      <NumberChip
        label="PRN"
        title="GPS L1 C/A satellite PRN"
        value={settings.prn}
        {...limitOf(limits, "prn")}
        onCommit={(prn) => onParams({ type: "gnss", settings: { ...settings, prn } })}
      />
      <HzChip
        label="Doppler"
        title="Symmetric Doppler search span"
        value={settings.doppler_hz}
        limit={limitOf(limits, "doppler_hz")}
        onCommit={(doppler_hz) => onParams({ type: "gnss", settings: { ...settings, doppler_hz } })}
      />
      <NumberChip
        label="Acquire"
        title="Correlation peak-to-floor acquisition threshold"
        value={settings.threshold}
        {...limitOf(limits, "threshold")}
        unit="× floor"
        onCommit={(threshold) => onParams({ type: "gnss", settings: { ...settings, threshold } })}
      />
    </>
  );
}
