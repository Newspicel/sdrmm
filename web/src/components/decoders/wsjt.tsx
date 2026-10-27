import { HzChip, limitOf, type Mode, NumberChip } from "./kit";

export function WsjtChips({ params, limits, onParams }: Mode<"ft8" | "ft4" | "wspr">) {
  const { type, settings } = params;
  return (
    <>
      <HzChip
        label="From"
        title="Lowest USB audio frequency searched"
        value={settings.audio_low_hz}
        limit={limitOf(limits, "audio_low_hz")}
        onCommit={(audio_low_hz) => onParams({ type, settings: { ...settings, audio_low_hz } })}
      />
      <HzChip
        label="To"
        title="Highest USB audio frequency searched"
        value={settings.audio_high_hz}
        limit={limitOf(limits, "audio_high_hz")}
        onCommit={(audio_high_hz) => onParams({ type, settings: { ...settings, audio_high_hz } })}
      />
      <NumberChip
        label="Candidates"
        title="Maximum synchronized signals tried per decode pass"
        value={settings.max_candidates}
        {...limitOf(limits, "max_candidates")}
        onCommit={(max_candidates) => onParams({ type, settings: { ...settings, max_candidates } })}
      />
    </>
  );
}
