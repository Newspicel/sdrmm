import { Play, Square } from "lucide-react";
import { Button } from "../../components/BaseControls";
import { BTN, ICON_BTN } from "../../components/controls";
import { meterTone } from "../../components/dbfs";
import { FaceFault } from "../../components/face/Fault";
import { GainMeter, MeterRow } from "../../components/face/Meter";
import { FaceStats, Stat } from "../../components/face/Stats";
import { Icon } from "../../components/Icon";
import { Tip } from "../../components/Tip";
import { monitorKey } from "../../lib/audio/monitor";
import { resumeAudioOutput } from "../../lib/audio/sink";
import { useChannelAudio } from "../../lib/audio/useChannelAudio";
import { type Input, useInputs } from "../../lib/resolved";
import type { PatchNode } from "../../lib/types";
import { useWorkspaceContext } from "../context";
import { AudioSpectrogramView } from "./AudioSpectrogramView";
import { FaceBody, FaceEmpty, FaceFooter, NodeShell } from "./NodeShell";
import { inputKey } from "./sinkInputs";
import {
  type SpeakerHealth,
  speakerHealthShown,
  useAudioPeakDb,
  useSpeakerHealth,
} from "./speaker";

export function SpeakerFace({ node }: { node: PatchNode }) {
  const inputs = useInputs(node.id, "audio");
  const health = useSpeakerHealth(inputs, import.meta.env.DEV);
  return (
    <NodeShell node={node} title="Speaker" category="output">
      <FaceBody>
        {inputs.length === 0 ? (
          <FaceEmpty hint="Wire a channel's audio in" />
        ) : (
          inputs.map((input, index) => (
            <AudioInput
              key={inputKey(input)}
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
              className={BTN}
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

function AudioInput({ input, port }: { input: Input; port?: string }) {
  const workspace = useWorkspaceContext();
  const audio = useChannelAudio(workspace.socket, input.deviceSet, input.channel.id, input.fx);
  const active = audio.playing || audio.pending || audio.suspended;
  const source = monitorKey(input.deviceSet, input.channel.id, input.fx);
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
      <AudioSpectrogramView source={source} playing={audio.playing} />
      {audio.error !== null && <FaceFault message={audio.error} />}
    </div>
  );
}

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
