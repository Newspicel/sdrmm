import { useEffect } from "react";
import { Button } from "../../components/BaseControls";
import { FACE_BTN } from "../../components/controls";
import { formatPeak, meterTone, meterUnit } from "../../components/dbfs";
import { Chips } from "../../components/face/Chips";
import { FaceFault } from "../../components/face/Fault";
import { GainMeter, MeterBar, MeterRow } from "../../components/face/Meter";
import { FaceStats, Stat } from "../../components/face/Stats";
import { SettingRow } from "../../components/Settings";
import { useDebouncedCommit } from "../../components/useDebouncedCommit";
import { tapKey } from "../../lib/audio/engine";
import { resumeAudioOutput } from "../../lib/audio/sink";
import { audioEngine, useChannelAudio } from "../../lib/audio/useChannelAudio";
import { listOf } from "../../lib/remembered";
import { type Input, useInputs } from "../../lib/resolved";
import type { PatchNode } from "../../lib/types";
import { useWorkspaceContext } from "../context";
import { patchNode } from "../graph";
import { Inspect, InspectSection } from "../inspector";
import { useNodeRemembered } from "../useNodeRemembered";
import { AudioSpectrogramView } from "./AudioSpectrogramView";
import { FaceBody, FaceEmpty, FaceFooter, NodeShell } from "./NodeShell";
import { SpeakerOutputChip } from "./SpeakerOutputChip";
import { inputKey } from "./sinkInputs";
import { METER_FILL, PlayToggle } from "./soundControls";
import {
  type SpeakerHealth,
  speakerHealthShown,
  useAudioPeakDb,
  useSpeakerHealth,
} from "./speaker";
import {
  listenerOutputOf,
  serverOutputOf,
  speakerOutputOf,
  speakerVolumeOf,
  withSpeakerVolume,
} from "./speakerOutput";

const isSources = listOf((value: unknown): value is string => typeof value === "string");
const NOTHING_HEARD: readonly string[] = [];

export function SpeakerFace({ node }: { node: PatchNode }) {
  const inputs = useInputs(node.id, "audio");
  const health = useSpeakerHealth(node.id, inputs, import.meta.env.DEV);
  const output = speakerOutputOf(node);
  const listener = listenerOutputOf(output);
  const onServer = serverOutputOf(output) !== undefined;
  useEffect(() => audioEngine.setOutput(node.id, listener), [node.id, listener]);
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
              speaker={node}
              input={input}
              port={index === 0 ? "audio" : undefined}
              onServer={onServer}
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

/** The sources this browser listens to on a speaker, kept across reloads and switches. */
function useListening(node: string, source: string) {
  const [heard, setHeard] = useNodeRemembered(node, "speakerListening", NOTHING_HEARD, isSources);
  const listening = heard.includes(source);
  const listen = (on: boolean): void =>
    setHeard((held) =>
      on ? [...held.filter((s) => s !== source), source] : held.filter((s) => s !== source),
    );
  return { listening, listen };
}

/** The wire's fader, kept in the patch so the server card and every listener share it. */
function useVolume(speaker: PatchNode, input: Input, setLive: (volume: number) => void) {
  const workspace = useWorkspaceContext();
  const saved = speakerVolumeOf(speaker, input.node);
  const commit = useDebouncedCommit((volume) =>
    workspace.edit((snapshot) => ({
      ...snapshot,
      graph: patchNode(snapshot.graph, speaker.id, (current) =>
        withSpeakerVolume(current, input.node, volume),
      ),
    })),
  );
  useEffect(() => setLive(saved), [saved, setLive]);
  const change = (volume: number): void => {
    setLive(volume);
    commit.change(volume);
  };
  return { volume: commit.pending ?? saved, change };
}

function AudioInput({
  speaker,
  input,
  port,
  onServer,
}: {
  speaker: PatchNode;
  input: Input;
  port?: string;
  onServer: boolean;
}) {
  const workspace = useWorkspaceContext();
  const node = speaker.id;
  const tap = { node, port: "audio", source: input.source };
  const audio = useChannelAudio(workspace.socket, tap);
  const active = audio.playing || audio.pending || audio.suspended;
  const { listening, listen } = useListening(node, input.source);
  const fader = useVolume(speaker, input, audio.setVolume);
  const { start, stop } = audio;
  useEffect(() => {
    if (onServer) {
      stop();
    } else if (listening) {
      start();
    }
  }, [onServer, listening, start, stop]);
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
            fill={METER_FILL[meterTone(peakDb, false)]}
          />
        }
        readout={`${Math.round(fader.volume * 100)}%`}
        trailing={
          !onServer && (
            <PlayToggle
              active={active}
              onToggle={() => {
                audio.resumeOutput();
                listen(!active);
                if (active) {
                  audio.stop();
                } else {
                  audio.start();
                }
              }}
            />
          )
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
                value={fader.volume}
                peakDb={peakDb ?? null}
                tone={meterTone(peakDb, false)}
                onChange={fader.change}
              />
            </SettingRow>
          </Chips>
        </InspectSection>
      </Inspect>
      <AudioSpectrogramView
        node={node}
        port="audio"
        source={input.source}
        playing={onServer || audio.playing}
      />
      {audio.error !== null && <FaceFault message={audio.error} />}
    </div>
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
