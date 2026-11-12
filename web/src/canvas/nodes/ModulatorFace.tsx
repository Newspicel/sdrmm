import { useMutation } from "@tanstack/react-query";
import { useState } from "react";
import { Button } from "../../components/BaseControls";
import { BTN_PRIMARY } from "../../components/controls";
import { Chips, ChoiceChip } from "../../components/face/Chips";
import { SettingsForm } from "../../components/SettingsForm";
import { nodeAction } from "../../lib/api";
import { clearAction, failAction } from "../../lib/refusals";
import type { Settings } from "../../lib/settingsForm";
import type { ParamChoice, PatchNode, PatchNodeOf } from "../../lib/types";
import { useWorkspaceContext } from "../context";
import { patchNode } from "../graph";
import { FaceBody, FaceEmpty, FaceFooter, NodeShell } from "./NodeShell";
import {
  modulatorSettings,
  modulatorTypes,
  withModulatorSettings,
  withModulatorType,
} from "./transmit";

const NO_CHOICES: readonly ParamChoice[] = [];
const NO_MESSAGE: Settings = {};
const SEND_ACTION = "Send";

export function ModulatorFace({ node }: { node: PatchNode }) {
  if (node.kind !== "modulator") {
    return null;
  }
  return <ModulatorNodeFace node={node} />;
}

function ModulatorNodeFace({ node }: { node: PatchNodeOf<"modulator"> }) {
  const workspace = useWorkspaceContext();
  const types = modulatorTypes(workspace.context.channelTypes);
  const typeId = node.data.channel_type;
  const descriptor = types.find((type) => type.type_id === typeId);
  const [drafts, setDrafts] = useState<Readonly<Record<string, Settings>>>({});
  const message = drafts[typeId] ?? NO_MESSAGE;
  const form = descriptor?.message ?? [];
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
    <NodeShell node={node} title={descriptor?.name ?? "Modulator"} category="channel">
      <FaceBody>
        <Chips>
          <ChoiceChip
            label="Mode"
            title="What it modulates"
            value={typeId}
            quiet={descriptor === undefined}
            options={types.map((type) => ({ value: type.type_id, label: type.name }))}
            onChange={(next) => edit((current) => withModulatorType(current, next))}
          />
          {descriptor !== undefined && (
            <SettingsForm
              form={descriptor.settings ?? []}
              settings={modulatorSettings(node.data.settings, descriptor)}
              choices={NO_CHOICES}
              onSettings={(settings) => edit((current) => withModulatorSettings(current, settings))}
            />
          )}
        </Chips>
        {descriptor === undefined && <FaceEmpty hint="Pick a mode" />}
        {form.length > 0 && (
          <Chips>
            <SettingsForm
              form={form}
              settings={message}
              choices={NO_CHOICES}
              onSettings={(next) => setDrafts((held) => ({ ...held, [typeId]: next }))}
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
