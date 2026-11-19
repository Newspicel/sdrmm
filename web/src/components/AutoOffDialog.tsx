import {
  Dialog,
  DialogBackdrop,
  DialogClose,
  DialogContent,
  DialogDescription,
  DialogTitle,
} from "@ui-sdk/react/dialog";
import { useState } from "react";
import { cancelLeave, confirmLeave, useAutoOff } from "../lib/autoOff";
import { Button } from "./BaseControls";
import { Checkbox } from "./Checkbox";
import { BTN_PRIMARY, BTN_QUIET, DIALOG_TITLE, SURFACE } from "./controls";
import { pageBody } from "./pageBody";

export function AutoOffDialog() {
  const open = useAutoOff((state) => state.pending !== null);
  const [neverAgain, setNeverAgain] = useState(false);
  return (
    <Dialog
      container={pageBody()}
      open={open}
      onOpenChange={(next) => {
        if (!next) {
          cancelLeave();
        }
      }}
    >
      <DialogBackdrop className="fixed inset-0 z-40 bg-bg/85 data-ending-style:hidden" />
      <DialogContent
        className={`${SURFACE} fixed top-1/2 data-ending-style:hidden left-1/2 z-40 w-full max-w-sm -translate-x-1/2 -translate-y-1/2 p-4`}
      >
        <DialogTitle className={DIALOG_TITLE}>Leave Auto tuning?</DialogTitle>
        <DialogDescription className="mt-3 flex flex-col gap-2 text-xs text-ink-dim">
          <span>
            On Auto, the radio follows your decoders. Set a decoder's frequency and the radio moves
            to cover it.
          </span>
          <span>
            On Manual, the radio stays where you put it. Decoders outside its window stop.
          </span>
        </DialogDescription>
        <label className="mt-3 flex items-center gap-2 text-xs text-ink-dim">
          <Checkbox checked={neverAgain} onChange={setNeverAgain} />
          Don't show again
        </label>
        <div className="mt-4 flex justify-end gap-2 border-t border-line pt-3">
          <Button type="button" className={BTN_QUIET} onClick={() => confirmLeave(neverAgain)}>
            Switch to Manual
          </Button>
          <DialogClose className={BTN_PRIMARY}>Stay on Auto</DialogClose>
        </div>
      </DialogContent>
    </Dialog>
  );
}
