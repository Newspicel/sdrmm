import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { examplesQuery, importExample, patchCatalogQuery, WORKSPACES_KEY } from "../lib/api";
import { pushToast } from "../lib/toasts";
import type { ExampleInfo } from "../lib/types";
import { Button } from "./BaseControls";
import { BTN_SM, CHIP_READOUT } from "./controls";
import { exampleNeeds, examplesHint, kindLabels } from "./examples";
import { List, ListRow, Panel, PanelHint } from "./ListPanel";

export function ExamplesPanel({ onImported }: { onImported: (id: number) => void }) {
  const queryClient = useQueryClient();
  const examples = useQuery(examplesQuery());
  const catalog = useQuery(patchCatalogQuery());
  const list = examples.data?.examples ?? [];

  const importMut = useMutation({
    mutationFn: (example: ExampleInfo) => importExample(example.id),
    onSuccess: (id, example) => {
      pushToast(`${example.name} imported`);
      onImported(id);
    },
    onError: (e) => pushToast(e.message),
    onSettled: () => void queryClient.invalidateQueries({ queryKey: WORKSPACES_KEY }),
  });

  const hint = examplesHint(examples.isPending, examples.error);

  return (
    <Panel>
      {hint !== null && <PanelHint>{hint}</PanelHint>}
      <List>
        {list.map((example) => (
          <ListRow
            key={example.id}
            primary={example.name}
            badge={<span className="legend shrink-0 tabular-nums">{exampleNeeds(example)}</span>}
            secondary={example.description}
            actions={
              <Button
                type="button"
                className={BTN_SM}
                disabled={importMut.isPending}
                title={`Import ${example.name} as a new workspace`}
                onClick={() => importMut.mutate(example)}
              >
                Import
              </Button>
            }
          >
            <div className="flex flex-wrap gap-1">
              {kindLabels(example, catalog.data).map((label) => (
                <span key={label} className={CHIP_READOUT}>
                  {label}
                </span>
              ))}
            </div>
          </ListRow>
        ))}
      </List>
    </Panel>
  );
}
