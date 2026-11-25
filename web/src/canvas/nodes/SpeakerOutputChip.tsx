import { useQuery } from "@tanstack/react-query";
import { Button } from "../../components/BaseControls";
import { BTN } from "../../components/controls";
import { ChoiceChip } from "../../components/face/Chips";
import { SettingRow } from "../../components/Settings";
import { serverOutputsQuery } from "../../lib/api";
import { outputsSupported, revealOutputNames } from "../../lib/audio/outputs";
import type { PatchNode, SpeakerOutput } from "../../lib/types";
import { useWorkspaceNow } from "../context";
import { patchNode } from "../graph";
import {
  choiceOf,
  listenerOutputOf,
  outputMissing,
  outputOfChoice,
  SYSTEM_OUTPUT,
  serverLabel,
  serverOutputOf,
  speakerOutputOf,
  useAudioOutputs,
  withSpeakerOutput,
} from "./speakerOutput";

const TITLE = "Where this speaker plays. Server plays on the server's sound card";

interface Choice {
  readonly value: string;
  readonly label: string;
}

export function choicesOf(
  listener: readonly string[],
  server: readonly string[] | null,
  output: SpeakerOutput | null,
  missing: boolean,
): Choice[] {
  const choices: Choice[] = [
    { value: choiceOf(null), label: SYSTEM_OUTPUT },
    ...listener.map((name) => ({ value: choiceOf(name), label: name })),
  ];
  if (server !== null) {
    choices.push({ value: choiceOf({ server: null }), label: serverLabel(null) });
    choices.push(
      ...server.map((name) => ({ value: choiceOf({ server: name }), label: serverLabel(name) })),
    );
  }
  const current = choiceOf(output);
  if (!choices.some((choice) => choice.value === current)) {
    const name = listenerOutputOf(output) ?? serverLabel(serverOutputOf(output) ?? null);
    choices.push({ value: current, label: missing ? `${name} (not connected)` : name });
  }
  return choices;
}

function titleOf(output: SpeakerOutput | null, missing: boolean, serverError: string | null) {
  if (missing) {
    return `${listenerOutputOf(output) ?? serverOutputOf(output)} is not connected`;
  }
  if (serverError !== null) {
    return `${TITLE}. ${serverError}`;
  }
  return TITLE;
}

export function SpeakerOutputChip({ node }: { node: PatchNode }) {
  const now = useWorkspaceNow();
  const { listed, refresh } = useAudioOutputs();
  const served = useQuery(serverOutputsQuery());
  const output = speakerOutputOf(node);
  const server = served.data?.outputs ?? null;
  const serverName = serverOutputOf(output);
  const missing =
    serverName === undefined
      ? outputMissing(listenerOutputOf(output), listed)
      : serverName !== null && server !== null && !server.includes(serverName);
  const pick = (next: string) => {
    now().edit((snapshot) => ({
      ...snapshot,
      graph: patchNode(snapshot.graph, node.id, (current) =>
        withSpeakerOutput(current, outputOfChoice(next)),
      ),
    }));
  };
  const names = listed?.outputs.map((candidate) => candidate.name) ?? [];
  const serverError = served.error === null ? null : served.error.message;
  return (
    <>
      <ChoiceChip
        label="Out"
        value={choiceOf(output)}
        options={choicesOf(names, server, output, missing)}
        title={titleOf(output, missing, serverError)}
        quiet={output === null}
        tone={missing ? "warn" : undefined}
        disabled={!outputsSupported() && server === null}
        onChange={pick}
      />
      {listed !== null && !listed.named && (
        <SettingRow label="Names">
          <Button
            type="button"
            className={BTN}
            title="The browser names devices once the page may use the microphone"
            onClick={() => {
              revealOutputNames()
                .then(refresh)
                .catch((err: unknown) => console.error("naming audio outputs failed:", err));
            }}
          >
            Show
          </Button>
        </SettingRow>
      )}
    </>
  );
}
