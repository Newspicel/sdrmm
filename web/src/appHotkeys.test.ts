import { describe, expect, it, vi } from "vitest";
import { type NodeHotkeys, nodeHotkeys, PICK_A_DECODER, PICK_A_RADIO } from "./appHotkeys";
import type { PatchNode } from "./lib/types";

const DEVICE = { id: "dev", kind: "device", data: {}, position: { x: 0, y: 0 } } as PatchNode;
const NFM = {
  id: "voice",
  kind: "channel",
  data: { channel_type: "nfm" },
  position: { x: 0, y: 0 },
} as PatchNode;

function hotkeys(over: Partial<NodeHotkeys>) {
  const calls = {
    tuneNode: vi.fn(),
    applyEdit: vi.fn(),
    retype: vi.fn(async () => {}),
    notify: vi.fn(),
  };
  const actions = nodeHotkeys({
    selectedNode: null,
    selectedHz: null,
    stepHz: 12_500,
    ...calls,
    ...over,
  });
  return { actions, ...calls };
}

describe("node hotkeys", () => {
  it("do nothing with nothing selected and say why", () => {
    const { actions, tuneNode, applyEdit, retype, notify } = hotkeys({});

    actions.tune(1);
    actions.cycleMode(1);
    actions.adjustSquelch(2);
    actions.toggleSquelch();

    expect(tuneNode).not.toHaveBeenCalled();
    expect(applyEdit).not.toHaveBeenCalled();
    expect(retype).not.toHaveBeenCalled();
    expect(notify.mock.calls).toEqual([
      [PICK_A_RADIO],
      [PICK_A_DECODER],
      [PICK_A_DECODER],
      [PICK_A_DECODER],
    ]);
  });

  it("tune the selected node by steps and leave locks to the server", () => {
    const { actions, tuneNode, notify } = hotkeys({ selectedNode: DEVICE, selectedHz: 100e6 });

    actions.tune(-2);

    expect(tuneNode).toHaveBeenCalledWith("dev", 100e6 - 25_000);
    expect(notify).not.toHaveBeenCalled();
  });

  it("retype the selected decoder through the server", () => {
    const { actions, retype } = hotkeys({ selectedNode: NFM, selectedHz: 145e6 });

    actions.cycleMode(1);

    expect(retype).toHaveBeenCalledWith("voice", "wfm");
  });

  it("squelch only a selected decoder", () => {
    const { actions, applyEdit, notify } = hotkeys({ selectedNode: DEVICE, selectedHz: 1 });
    actions.toggleSquelch();
    expect(applyEdit).not.toHaveBeenCalled();
    expect(notify).toHaveBeenCalledWith(PICK_A_DECODER);

    const decoder = hotkeys({ selectedNode: NFM, selectedHz: 1 });
    decoder.actions.toggleSquelch();
    expect(decoder.applyEdit).toHaveBeenCalledWith("voice", expect.any(Function));
  });
});
