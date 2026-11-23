import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it } from "vitest";
import type { PatchNode } from "../../lib/types";
import { renderFace } from "../../test/faceHarness";
import { TalkerRow, VoiceChatFace } from "./VoiceChatFace";

const GATE = { mode: "voice_activity", threshold_db: -35, hang_ms: 600, volume: 1 } as const;

const NODE = {
  id: "vc",
  kind: "voice_chat",
  position: { x: 0, y: 0 },
  data: { account: "gone", channel: "", allowed: [], from_channel: GATE, to_channel: GATE },
} as PatchNode;

function render(inspected = false): string {
  return renderFace(VoiceChatFace, NODE, {
    graph: { nodes: [NODE], edges: [] },
    resolved: null,
    inspected,
  });
}

describe("VoiceChatFace", () => {
  it("keeps its settings in the sidebar", () => {
    expect(render()).not.toContain("Chat account, from the Accounts list");
  });

  it("shows an account it cannot see as unknown, never as a fault", () => {
    const html = render(true);
    expect(html).toContain('aria-label="Chat account, from the Accounts list"');
    expect(html).toContain(">unknown<");
    expect(html).not.toContain('role="alert"');
  });

  it("keeps its meters and talking list before any status arrives", () => {
    const html = render();
    expect(html).toContain('aria-label="To channel level"');
    expect(html).toContain('aria-label="From channel level"');
    expect(html).toContain('aria-valuenow="0"');
    expect(html).toContain(">Talking<");
  });

  it("offers each direction voice activity or continuous, never push to talk", () => {
    const html = render(true);
    expect(html).toContain("From channel: when audio is sent");
    expect(html).toContain("To channel: when audio is sent");
    expect(html).toContain("Voice activity");
    expect(html).not.toContain("Push to talk");
  });

  it("offers a volume slider per direction, from silence to double", () => {
    const html = render(true);
    for (const name of ["From channel", "To channel"]) {
      expect(html).toContain(`aria-label="${name} volume"`);
    }
    expect(html).toContain('max="200"');
    expect(html).toContain("100 %");
  });

  it("offers an id to copy only for a verified talker", () => {
    const ann = { id: "42", name: "Ann", allowed: true, verified: true };
    expect(renderToStaticMarkup(<TalkerRow talker={ann} />)).toContain("Copy Ann id");
    const standIn = { id: "ssrc 7", name: "ssrc 7", allowed: false, verified: false };
    expect(renderToStaticMarkup(<TalkerRow talker={standIn} />)).not.toContain("Copy");
  });
});
