import { useEffect } from "react";
import { formatPeak, meterTone, meterUnit } from "../../components/dbfs";
import { Chips } from "../../components/face/Chips";
import { FaceFault } from "../../components/face/Fault";
import { GainMeter, MeterBar, MeterRow } from "../../components/face/Meter";
import { SettingRow } from "../../components/Settings";
import { tapKey } from "../../lib/audio/engine";
import { useChannelAudio } from "../../lib/audio/useChannelAudio";
import { isBoolean, within } from "../../lib/remembered";
import type { Input } from "../../lib/resolved";
import { useWorkspaceSelect } from "../context";
import { Inspect, InspectSection } from "../inspector";
import { useNodeRemembered } from "../useNodeRemembered";
import { METER_FILL, PlayToggle } from "./soundControls";
import { useAudioPeakDb } from "./speaker";

const isVolume = within(0, 1);

/** The sound a video wire carries with its pictures, heard in this browser. */
export function VideoSound({ node, input, name }: { node: string; input: Input; name: string }) {
  const socket = useWorkspaceSelect((workspace) => workspace.socket);
  const tap = { node, port: "video", source: input.source };
  const audio = useChannelAudio(socket, tap);
  const [listening, setListening] = useNodeRemembered(
    node,
    `videoListening:${input.source}`,
    false,
    isBoolean,
  );
  const [volume, setVolume] = useNodeRemembered(node, `videoVolume:${input.source}`, 1, isVolume);
  const { start, stop, setVolume: setLive } = audio;
  useEffect(() => setLive(volume), [volume, setLive]);
  useEffect(() => {
    if (listening) {
      start();
    }
  }, [listening, start]);
  const active = audio.playing || audio.pending || audio.suspended;
  const peakDb = useAudioPeakDb(tapKey(tap), audio.playing);
  const toggle = (): void => {
    audio.resumeOutput();
    setListening(!active);
    if (active) {
      stop();
    } else {
      start();
    }
  };
  return (
    <div className="flex flex-col gap-1 border-t border-line p-2">
      <MeterRow
        label="Sound"
        meter={
          <MeterBar
            label={`${name} sound level`}
            value={meterUnit(peakDb)}
            valueText={formatPeak(peakDb)}
            fill={METER_FILL[meterTone(peakDb, false)]}
          />
        }
        readout={`${Math.round(volume * 100)}%`}
        trailing={<PlayToggle active={active} onToggle={toggle} />}
      />
      <Inspect node={node}>
        <InspectSection label={`${name} sound`}>
          <Chips inset="none">
            <SettingRow label="Volume">
              <GainMeter
                label={`${name} volume`}
                className="min-w-0 flex-1"
                min={0}
                max={1}
                step={0.01}
                value={volume}
                peakDb={peakDb ?? null}
                tone={meterTone(peakDb, false)}
                onChange={setVolume}
              />
            </SettingRow>
          </Chips>
        </InspectSection>
      </Inspect>
      {audio.error !== null && <FaceFault message={audio.error} />}
    </div>
  );
}
