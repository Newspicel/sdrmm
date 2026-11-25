import { useMutation, useQueryClient } from "@tanstack/react-query";
import { Button } from "../components/BaseControls";
import { BTN_QUIET } from "../components/controls";
import { createWorkspace, WORKSPACES_KEY, wiringVerdicts } from "../lib/api";
import { useT } from "../lib/i18n/translate";
import { uiScale } from "../lib/prefs";
import { pushToast } from "../lib/toasts";
import { useWorkspaceContext } from "./context";
import { type TestContext, testWorkspaces } from "./testWorkspace";

/** Draws every node face in every state into new workspaces and opens the first. */
export function TestWorkspaceButton({ onCreated }: { onCreated: (id: number) => void }) {
  const t = useT();
  const queryClient = useQueryClient();
  const { context } = useWorkspaceContext();
  const create = useMutation({
    mutationFn: () => createAll(context),
    onSuccess: (id) => {
      if (id !== undefined) {
        onCreated(id);
      }
    },
    onError: (error) => pushToast(error.message),
    onSettled: () => void queryClient.invalidateQueries({ queryKey: WORKSPACES_KEY }),
  });
  return (
    <Button
      type="button"
      className={`${BTN_QUIET} justify-center`}
      title={t("workspace.testTitle")}
      disabled={create.isPending}
      onClick={() => create.mutate()}
    >
      {t("workspace.test")}
    </Button>
  );
}

async function createAll(context: TestContext): Promise<number | undefined> {
  const ids: number[] = [];
  for (const { name, snapshot } of await testWorkspaces(context, wiringVerdicts, uiScale())) {
    ids.push(await createWorkspace(name, snapshot));
  }
  return ids[0];
}
