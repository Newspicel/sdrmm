import { useState } from "react";
import { FaceStats, Stat } from "../../components/face/Stats";
import { type VideoSignal, VideoView, videoSignalText } from "../../components/VideoView";
import { useInputs } from "../../lib/resolved";
import type { PatchNode } from "../../lib/types";
import { FaceBody, FaceEmpty, FaceFooter, NodeShell } from "./NodeShell";
import { useInputName } from "./sinkInputs";

export function VideoFace({ node }: { node: PatchNode }) {
  const inputs = useInputs(node.id, "video");
  const nameOf = useInputName();
  const [signals, setSignals] = useState<Readonly<Record<string, VideoSignal | null>>>({});
  return (
    <NodeShell node={node} title="Video" category="output">
      <FaceBody>
        {inputs.length === 0 ? (
          <FaceEmpty hint="Wire a video channel's picture in" />
        ) : (
          inputs.map((input) => (
            <VideoView
              key={input.node}
              scope={{ deviceSet: input.deviceSet, channel: input.channel.id }}
              onSignal={(signal) => setSignals((held) => ({ ...held, [input.node]: signal }))}
            />
          ))
        )}
      </FaceBody>
      {inputs.length > 0 && (
        <FaceFooter>
          <FaceStats>
            {inputs.map((input) => {
              const signal = signals[input.node] ?? null;
              return (
                <Stat
                  key={input.node}
                  label={inputs.length > 1 ? nameOf(input) : "Picture"}
                  title="Decoded picture size and sync"
                  tone={signal?.live === true ? undefined : "warn"}
                >
                  {videoSignalText(signal)}
                </Stat>
              );
            })}
          </FaceStats>
        </FaceFooter>
      )}
    </NodeShell>
  );
}
