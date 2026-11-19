import { Button } from "../../components/BaseControls";
import { BTN } from "../../components/controls";
import { ChoiceChip } from "../../components/face/Chips";
import { SettingRow } from "../../components/Settings";
import { outputsSupported, revealOutputNames } from "../../lib/audio/outputs";
import type { PatchNode } from "../../lib/types";
import { useWorkspaceContext } from "../context";
import { patchNode } from "../graph";
import {
  outputMissing,
  SYSTEM_OUTPUT,
  speakerOutputOf,
  useAudioOutputs,
  withSpeakerOutput,
} from "./speakerOutput";

const TITLE = "Where this speaker plays. Pick a virtual device to feed another program";

/** The system output as a choice, since a device name is never empty. */
const SYSTEM = "";

function titleOf(output: string | null, missing: boolean): string {
  if (!outputsSupported()) {
    return "This browser plays only on the system output";
  }
  return missing ? `${output} is not connected` : TITLE;
}

function choicesOf(names: readonly string[], output: string | null, missing: boolean) {
  const choices = [
    { value: SYSTEM, label: SYSTEM_OUTPUT },
    ...names.map((name) => ({ value: name, label: name })),
  ];
  if (output !== null && !names.includes(output)) {
    choices.push({ value: output, label: missing ? `${output} (not connected)` : output });
  }
  return choices;
}

export function SpeakerOutputChip({ node }: { node: PatchNode }) {
  const workspace = useWorkspaceContext();
  const { listed, refresh } = useAudioOutputs();
  const output = speakerOutputOf(node);
  const missing = outputMissing(output, listed);
  const pick = (next: string) => {
    workspace.edit((snapshot) => ({
      ...snapshot,
      graph: patchNode(snapshot.graph, node.id, (current) =>
        withSpeakerOutput(current, next === SYSTEM ? null : next),
      ),
    }));
  };
  const names = listed?.outputs.map((candidate) => candidate.name) ?? [];
  return (
    <>
      <ChoiceChip
        label="Out"
        value={output ?? SYSTEM}
        options={choicesOf(names, output, missing)}
        title={titleOf(output, missing)}
        quiet={output === null}
        tone={missing ? "warn" : undefined}
        disabled={!outputsSupported()}
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
