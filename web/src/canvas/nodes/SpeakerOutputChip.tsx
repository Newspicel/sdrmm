import { Button } from "../../components/BaseControls";
import { listItem } from "../../components/controls";
import { SettingChip } from "../../components/face/Chips";
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

function titleOf(output: string | null, missing: boolean): string {
  if (!outputsSupported()) {
    return "This browser plays only on the system output";
  }
  return missing ? `${output} is not connected` : TITLE;
}

export function SpeakerOutputChip({ node }: { node: PatchNode }) {
  const workspace = useWorkspaceContext();
  const { listed, refresh } = useAudioOutputs();
  const output = speakerOutputOf(node);
  const missing = outputMissing(output, listed);
  const pick = (next: string | null) => {
    workspace.edit((snapshot) => ({
      ...snapshot,
      graph: patchNode(snapshot.graph, node.id, (current) => withSpeakerOutput(current, next)),
    }));
  };
  const names = listed?.outputs.map((candidate) => candidate.name) ?? [];
  const choices: (string | null)[] = [null, ...names];
  if (output !== null && !names.includes(output)) {
    choices.push(output);
  }
  return (
    <SettingChip
      label="Out"
      value={output ?? SYSTEM_OUTPUT}
      title={titleOf(output, missing)}
      quiet={output === null}
      tone={missing ? "warn" : undefined}
      disabled={!outputsSupported()}
      padded={false}
      width="w-64"
    >
      {(close) => (
        <div role="listbox" aria-label={TITLE} className="flex flex-col py-1">
          {choices.map((choice) => (
            <Button
              key={choice ?? ""}
              type="button"
              role="option"
              aria-selected={choice === output}
              className={listItem(choice === output, false)}
              onClick={() => {
                if (choice !== output) pick(choice);
                close();
              }}
            >
              <span className="truncate">{choice ?? SYSTEM_OUTPUT}</span>
            </Button>
          ))}
          {listed !== null && !listed.named && (
            <Button
              type="button"
              className={listItem(false, false)}
              title="The browser names devices once the page may use the microphone"
              onClick={() => {
                revealOutputNames()
                  .then(refresh)
                  .catch((err: unknown) => console.error("naming audio outputs failed:", err));
              }}
            >
              Show device names
            </Button>
          )}
        </div>
      )}
    </SettingChip>
  );
}
