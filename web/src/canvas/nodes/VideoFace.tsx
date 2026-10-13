import { useState } from "react";
import { FaceStats, Stat } from "../../components/face/Stats";
import { type VideoSignal, VideoView, videoSignalText } from "../../components/VideoView";
import { capturedImageUrl } from "../../lib/api";
import { useInputs } from "../../lib/resolved";
import { useSinkFeed } from "../../lib/sinks";
import type { CapturedImage, PatchNode } from "../../lib/types";
import { FaceBody, FaceEmpty, FaceFooter, NodeShell } from "./NodeShell";
import { useInputName } from "./sinkInputs";

export function VideoFace({ node }: { node: PatchNode }) {
  const inputs = useInputs(node.id, "video");
  const nameOf = useInputName();
  const [signals, setSignals] = useState<Readonly<Record<string, VideoSignal | null>>>({});
  const { pictures } = useSinkFeed(node.id);
  return (
    <NodeShell node={node} title="Video" category="output">
      <FaceBody>
        {inputs.length === 0 ? (
          <FaceEmpty hint="Wire a video channel's picture in" />
        ) : (
          inputs.map((input) => (
            <VideoView
              key={input.source}
              scope={{ node: node.id, port: "video", source: input.source }}
              onSignal={(signal) => setSignals((held) => ({ ...held, [input.node]: signal }))}
            />
          ))
        )}
        {pictures.length > 0 && <Gallery pictures={pictures} />}
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

const GALLERY = 6;

function Gallery({ pictures }: { pictures: readonly CapturedImage[] }) {
  return (
    <div className="grid grid-cols-3 gap-1 border-t border-line p-2">
      {pictures.slice(0, GALLERY).map((picture) => (
        <GalleryItem key={picture.id} picture={picture} />
      ))}
    </div>
  );
}

function GalleryItem({ picture }: { picture: CapturedImage }) {
  const title = `${picture.mode} ${picture.width} × ${picture.height}`;
  if (picture.image == null) {
    return (
      <div className="legend truncate text-2xs text-warn" title={picture.image_error ?? title}>
        {picture.mode}
      </div>
    );
  }
  const url = capturedImageUrl(picture.image.url);
  return (
    <a href={url} target="_blank" rel="noreferrer" title={title}>
      <img src={url} alt={title} className="aspect-4/3 w-full rounded-inset object-cover" />
    </a>
  );
}
