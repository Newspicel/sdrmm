import { useQuery } from "@tanstack/react-query";
import { ModelDownload } from "../../components/ModelDownload";
import {
  deleteSpeechModel,
  downloadSpeechModel,
  SPEECH_MODELS_KEY,
  speechModelsQuery,
} from "../../lib/api";
import { useTranscriberStore } from "../../lib/transcriber";
import type { Field, PatchNode, SpeechModel } from "../../lib/types";
import { NodeForm, StatusGrid } from "./NodeForm";
import { FaceBody, FaceStanding, NodeShell } from "./NodeShell";
import { useUnwired, type Want } from "./needs";

const NO_READOUTS: readonly Field[] = [];
const WANTS: readonly Want[] = [{ port: "events" }];
const DEFAULT_MODEL: SpeechModel = "parakeet-tdt-0.6b-v3";

export function TranscriberFace({ node }: { node: PatchNode }) {
  const status = useTranscriberStore((store) => store.byNode[node.id]);
  const unwired = useUnwired(node.id, WANTS);
  const models = useQuery(speechModelsQuery());
  if (node.kind !== "transcriber") {
    return null;
  }
  const model = node.data.settings?.model ?? DEFAULT_MODEL;
  const file = models.data?.models.find((entry) => entry.model === model);
  return (
    <NodeShell node={node} title="Transcriber" category="tool" live={(status?.queued ?? 0) > 0}>
      <FaceBody>
        <NodeForm
          node={node}
          lead={
            file === undefined ? undefined : (
              <ModelDownload
                status={file}
                queryKey={SPEECH_MODELS_KEY}
                download={downloadSpeechModel}
                remove={deleteSpeechModel}
              />
            )
          }
        />
        <StatusGrid readouts={status?.readouts ?? NO_READOUTS} />
        <FaceStanding unwired={unwired} error={status?.error} />
      </FaceBody>
    </NodeShell>
  );
}
