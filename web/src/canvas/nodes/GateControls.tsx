import type { Options } from "../../components/controls";
import { ChoiceChip, NumberChip } from "../../components/face/Chips";
import { GainMeter, MeterRow, MeterSlider } from "../../components/face/Meter";
import { SettingRow } from "../../components/Settings";
import { useDebouncedCommit } from "../../components/useDebouncedCommit";
import { levelUnit } from "../../lib/levels";
import { VOICE_CHAT_LIMITS } from "../../lib/limits";
import type { SendMode, VoiceGateSettings } from "../../lib/types";

const THRESHOLD = VOICE_CHAT_LIMITS.threshold_db;
const HANG = VOICE_CHAT_LIMITS.hang_ms;
const VOLUME = VOICE_CHAT_LIMITS.volume;
const PERCENT = 100;

/** How far up the gate's scale a level sits; no level reads as the floor. */
export function gateUnit(db: number | null | undefined): number {
  return levelUnit(db ?? Number.NEGATIVE_INFINITY, THRESHOLD.min);
}

/** One voice gate: what sends, the level that opens it over the live level, its hang and, when
 * `volume` is set, the gain it applies. */
export function GateControls({
  name,
  gate,
  modes,
  levelDb,
  open,
  volume = false,
  onEdit,
}: {
  name: string;
  gate: VoiceGateSettings;
  modes: Options<SendMode>;
  levelDb: number | null | undefined;
  open: boolean;
  volume?: boolean;
  onEdit: (next: Partial<VoiceGateSettings>) => void;
}) {
  const voiced = gate.mode === "voice_activity";
  const slider = useDebouncedCommit((threshold_db) => onEdit({ threshold_db }));
  const threshold = slider.pending ?? gate.threshold_db;
  const gain = useDebouncedCommit((percent) => onEdit({ volume: percent / PERCENT }));
  const percent = gain.pending ?? Math.round(gate.volume * PERCENT);
  return (
    <>
      <ChoiceChip
        label="Send"
        title={`${name}: when audio is sent`}
        value={gate.mode}
        options={modes}
        onChange={(mode) => onEdit({ mode })}
      />
      <MeterRow
        label="Level"
        title="Fill: audio level. Handle: where voice activity opens"
        meter={
          <MeterSlider
            label={`${name} voice level`}
            className="min-w-0 flex-1"
            min={THRESHOLD.min}
            max={THRESHOLD.max}
            step={1}
            value={threshold}
            disabled={!voiced}
            fill={{
              level: gateUnit(levelDb),
              className: open ? "bg-accent" : "bg-accent-dim",
            }}
            onChange={slider.change}
          />
        }
        readout={`${Math.round(threshold)} dB`}
      />
      <NumberChip
        label="Hang"
        title="How long voice activity stays open after the voice stops"
        value={gate.hang_ms}
        unit="ms"
        min={HANG.min}
        max={HANG.max}
        step={100}
        disabled={!voiced}
        onCommit={(hang_ms) => onEdit({ hang_ms })}
      />
      {volume && (
        <SettingRow label="Volume">
          <GainMeter
            label={`${name} volume`}
            className="min-w-0 flex-1"
            min={VOLUME.min * PERCENT}
            max={VOLUME.max * PERCENT}
            step={1}
            value={percent}
            onChange={gain.change}
          />
          <span className="tabular-nums">{percent} %</span>
        </SettingRow>
      )}
    </>
  );
}
