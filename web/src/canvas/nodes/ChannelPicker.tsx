import {
  Dialog,
  DialogBackdrop,
  DialogClose,
  DialogContent,
  DialogDescription,
  DialogTitle,
} from "@ui-sdk/react/dialog";
import { useState } from "react";
import { Form, Input } from "../../components/BaseControls";
import { BTN, DIALOG_TITLE, FIELD, SURFACE } from "../../components/controls";
import { pageBody } from "../../components/pageBody";
import { PaletteList } from "../NodePalette";
import { filterPalette, firstPaletteItem, type PaletteGroup, type PaletteItem } from "../palette";

export function ChannelPicker({
  title,
  note,
  groups,
  onChannel,
  onClose,
}: {
  title: string;
  note: string;
  groups: readonly PaletteGroup[];
  onChannel: (channelType: string) => void;
  onClose: () => void;
}) {
  const [query, setQuery] = useState("");
  const shown = filterPalette(groups, query);

  const choose = (item: PaletteItem | undefined): void => {
    if (item?.typeId !== undefined) {
      onChannel(item.typeId);
    }
  };

  return (
    <Dialog
      open
      container={pageBody()}
      onOpenChange={(open) => {
        if (!open) {
          onClose();
        }
      }}
    >
      <DialogBackdrop className="fixed inset-0 z-40 bg-bg/85 data-ending-style:hidden" />
      <DialogContent
        className={`${SURFACE} fixed top-1/2 data-ending-style:hidden left-1/2 z-40 flex max-h-[80vh] w-full max-w-lg -translate-x-1/2 -translate-y-1/2 flex-col p-4`}
      >
        <DialogTitle className={DIALOG_TITLE}>{title}</DialogTitle>
        <DialogDescription className="mt-1 font-mono text-xs tabular-nums text-ink-dim">
          {note}
        </DialogDescription>

        <Form
          className="mt-3 flex min-h-0 flex-1 flex-col"
          onSubmit={(event) => {
            event.preventDefault();
            choose(firstPaletteItem(shown));
          }}
        >
          <Input
            autoFocus
            className={`${FIELD} w-full shrink-0`}
            type="search"
            name="channel-mode-filter"
            aria-label="Search channel modes"
            placeholder="Name or family"
            value={query}
            onChange={(event) => setQuery(event.target.value)}
          />
          <div className="mt-2 min-h-0 overflow-y-auto">
            <PaletteList groups={shown} columns={2} onPick={choose} />
          </div>
        </Form>

        <div className="mt-4 flex shrink-0 justify-end">
          <DialogClose className={BTN}>Cancel</DialogClose>
        </div>
      </DialogContent>
    </Dialog>
  );
}
