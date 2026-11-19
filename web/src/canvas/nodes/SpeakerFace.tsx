import { Play, Square } from "lucide-react";
import { useEffect } from "react";
import { Button } from "../../components/BaseControls";
import { FACE_BTN, ICON_BTN } from "../../components/controls";
import { formatPeak, type MeterTone, meterTone, meterUnit } from "../../components/dbfs";
import { Chips } from "../../components/face/Chips";
import { FaceFault } from "../../components/face/Fault";
import { GainMeter, MeterBar, MeterRow } from "../../components/face/Meter";
import { FaceStats, Stat } from "../../components/face/Stats";
import { Icon } from "../../components/Icon";
import { SettingRow } from "../../components/Settings";
import { Tip } from "../../components/Tip";
import { tapKey } from "../../lib/audio/engine";
import { resumeAudioOutput } from "../../lib/audio/sink";
import { audioEngine, useChannelAudio } from "../../lib/audio/useChannelAudio";
import { type Input, useInputs } from "../../lib/resolved";
import type { PatchNode } from "../../lib/types";
import { useWorkspaceContext } from "../context";
import { Inspect, InspectSection } from "../inspector";
import { AudioSpectrogramView } from "./AudioSpectrogramView";
import { FaceBody, FaceEmpty, FaceFooter, NodeShell } from "./NodeShell";
import { SpeakerOutputChip } from "./SpeakerOutputChip";
import { inputKey } from "./sinkInputs";
import {
  type SpeakerHealth,
  speakerHealthShown,
  useAudioPeakDb,
  useSpeakerHealth,
} from "./speaker";
import { speakerOutputOf } from "./speakerOutput";

export function SpeakerFace({ node }: { node: PatchNode }) {
  const inputs = useInputs(node.id, "audio");
  const health = useSpeakerHealth(node.id, inputs, import.meta.env.DEV);
  const output = speakerOutputOf(node);
  useEffect(() => audioEngine.setOutput(node.id, output), [node.id, output]);
  return (
    <NodeShell node={node} title="Speaker" category="output">
      <Inspect node={node.id}>
        <InspectSection label="Output">
          <Chips inset="none">
            <SpeakerOutputChip node={node} />
          </Chips>
        </InspectSection>
      </Inspect>
      <FaceBody>
        {inputs.length === 0 ? (
          <FaceEmpty />
        ) : (
          inputs.map((input, index) => (
            <AudioInput
              key={inputKey(input)}
              node={node.id}
              input={input}
              port={index === 0 ? "audio" : undefined}
            />
          ))
        )}
      </FaceBody>
      {inputs.length > 0 && speakerHealthShown(health) && (
        <FaceFooter>
          <AudioHealth health={health} />
          {health.suspended && (
            <Button
              type="button"
              className={FACE_BTN}
              onClick={resumeAudioOutput}
              title="Audio output is suspended"
            >
              Resume audio
            </Button>
          )}
        </FaceFooter>
      )}
    </NodeShell>
  );
}

function AudioInput({ node, input, port }: { node: string; input: Input; port?: string }) {
  const workspace = useWorkspaceContext();
  const tap = { node, port: "audio", source: input.source };
  const audio = useChannelAudio(workspace.socket, tap);
  const active = audio.playing || audio.pending || audio.suspended;
  const source = tapKey(tap);
  const peakDb = useAudioPeakDb(source, audio.playing);
  const name =
    workspace.graph.nodes.find((n) => n.id === input.node)?.label ??
    input.channel.settings.params.type.toUpperCase();
  return (
    <div className="flex flex-col gap-1 border-b border-line p-2 last:border-b-0">
      <MeterRow
        label={
          <span className="legend truncate" title={name}>
            {name}
          </span>
        }
        port={port}
        meter={
          <MeterBar
            label={`${name} level`}
            value={meterUnit(peakDb)}
            valueText={formatPeak(peakDb)}
            fill={TONE[meterTone(peakDb, false)]}
          />
        }
        readout={`${Math.round(audio.volume * 100)}%`}
        trailing={
          <PlayToggle
            active={active}
            onToggle={() => {
              audio.resumeOutput();
              if (active) {
                audio.stop();
              } else {
                audio.start();
              }
            }}
          />
        }
      />
      <Inspect node={node}>
        <InspectSection label={name}>
          <Chips inset="none">
            <SettingRow label="Volume">
              <GainMeter
                label={`${name} volume`}
                className="min-w-0 flex-1"
                min={0}
                max={1}
                step={0.01}
                value={audio.volume}
                peakDb={peakDb ?? null}
                tone={meterTone(peakDb, false)}
                onChange={audio.setVolume}
              />
            </SettingRow>
          </Chips>
        </InspectSection>
      </Inspect>
      <AudioSpectrogramView
        node={node}
        port="audio"
        source={input.source}
        playing={audio.playing}
      />
      {audio.error !== null && <FaceFault message={audio.error} />}
    </div>
  );
}

const TONE: Record<MeterTone, string> = {
  ok: "bg-ok",
  warn: "bg-warn",
  danger: "bg-danger",
};

function PlayToggle({ active, onToggle }: { active: boolean; onToggle: () => void }) {
  return (
    <Tip
      text={active ? "Stop listening" : "Listen"}
      render={
        <Button
          type="button"
          className={`${ICON_BTN} ${active ? "bg-accent/15 text-accent" : ""}`}
          aria-label={active ? "Stop" : "Play"}
          aria-pressed={active}
          onClick={onToggle}
        />
      }
    >
      <Icon glyph={active ? Square : Play} size={14} filled={active} />
    </Tip>
  );
}

function AudioHealth({ health }: { health: SpeakerHealth }) {
  return (
    <FaceStats>
      {health.bufferedMs > 0 && (
        <Stat label="Buffer" title="Audio waiting for playback">
          {health.bufferedMs.toFixed(0)} ms
        </Stat>
      )}
      {health.trimmedMs > 0 && (
        <Stat label="Trimmed" title="Old audio discarded to stay live">
          {health.trimmedMs.toFixed(0)} ms
        </Stat>
      )}
      {health.droppedMs > 0 && (
        <Stat
          label="Dropped"
          tone="warn"
          title="Audio lost before playback: dropped at the radio, the encoder or the link, or decoded too late on this machine to be played."
        >
          {health.droppedMs.toFixed(0)} ms
        </Stat>
      )}
      {health.underruns > 0 && (
        <Stat
          label="Stalls"
          tone="warn"
          title="Audio arrived but playback ran dry before it could be played: this machine's scheduling or a clock the buffer could not track. The buffer holds more after each one."
        >
          {health.underruns}
        </Stat>
      )}
    </FaceStats>
  );
}
