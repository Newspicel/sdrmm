import { useMutation } from "@tanstack/react-query";
import { useState } from "react";
import { Button } from "../../components/BaseControls";
import { BTN_PRIMARY } from "../../components/controls";
import { Chips } from "../../components/face/Chips";
import { SettingsForm } from "../../components/SettingsForm";
import { nodeAction } from "../../lib/api";
import { clearAction, failAction } from "../../lib/refusals";
import { useFeeds } from "../../lib/resolved";
import type { Settings } from "../../lib/settingsForm";
import type { ParamChoice, PatchNode, PatchNodeOf } from "../../lib/types";
import { useWorkspaceContext } from "../context";
import { patchNode } from "../graph";
import { FaceBody, FaceEmpty, FaceFooter, NodeShell } from "./NodeShell";
import { encoderSettings, withEncoderSettings } from "./transmit";

const NO_CHOICES: readonly ParamChoice[] = [];
const NO_MESSAGE: Settings = {};
const SEND_ACTION = "Send";

export function EncoderFace({ node }: { node: PatchNode }) {
  if (node.kind !== "encoder") {
    return null;
  }
  return <EncoderNodeFace node={node} />;
}

function EncoderNodeFace({ node }: { node: PatchNodeOf<"encoder"> }) {
  const workspace = useWorkspaceContext();
  const descriptor = workspace.context.encoderTypes?.find(
    (type) => type.type_id === node.data.encoder_type,
  );
  const settings = encoderSettings(node.data.settings, descriptor);
  const [message, setMessage] = useState<Settings>(NO_MESSAGE);
  const form = descriptor?.message ?? [];
  const heard = useFeeds(node.id, "audio").length > 0;
  const takesAudio = (descriptor?.input ?? "audio") === "audio";
  const edit = (change: (current: PatchNode) => PatchNode): void =>
    workspace.edit((snapshot) => ({
      ...snapshot,
      graph: patchNode(snapshot.graph, node.id, change),
    }));
  const send = useMutation({
    mutationFn: () => nodeAction(node.id, { action: "send", with: { message } }),
    onSuccess: () => clearAction(node.id, SEND_ACTION),
    onError: (error: Error) => failAction(node.id, SEND_ACTION, error),
  });

  return (
    <NodeShell
      node={node}
      title={descriptor?.name ?? node.data.encoder_type}
      subtitle="Encoder"
      category="encoder"
    >
      <FaceBody>
        {descriptor === undefined && <FaceEmpty hint="Mode not loaded" />}
        {descriptor !== undefined && takesAudio && !heard && <FaceEmpty hint="Wire audio in" />}
        {descriptor !== undefined && (descriptor.settings ?? []).length > 0 && (
          <Chips>
            <SettingsForm
              form={descriptor.settings ?? []}
              settings={settings}
              choices={NO_CHOICES}
              onSettings={(next) => edit((current) => withEncoderSettings(current, next))}
            />
          </Chips>
        )}
        {form.length > 0 && (
          <Chips>
            <SettingsForm
              form={form}
              settings={message}
              choices={NO_CHOICES}
              onSettings={setMessage}
            />
          </Chips>
        )}
      </FaceBody>
      {form.length > 0 && (
        <FaceFooter>
          <Button
            type="button"
            className={BTN_PRIMARY}
            title="Sends this message once through the armed transmitter"
            disabled={send.isPending}
            onClick={() => send.mutate()}
          >
            Send
          </Button>
        </FaceFooter>
      )}
    </NodeShell>
  );
}
