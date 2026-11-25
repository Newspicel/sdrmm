import { describe, expect, it } from "vitest";
import type { EventOutputTarget, PatchNode } from "../../lib/types";
import { renderFace } from "../../test/faceHarness";
import { EventOutputFace } from "./EventOutputFace";

function output(target: EventOutputTarget): PatchNode {
  return { id: "out", kind: "event_output", data: { target }, position: { x: 0, y: 0 } };
}

function render(target: EventOutputTarget, inspected = true): string {
  const node = output(target);
  return renderFace(EventOutputFace, node, { graph: { nodes: [node], edges: [] }, inspected });
}

describe("EventOutputFace", () => {
  it("keeps its settings off the face and names the service there", () => {
    const html = render({ service: "mqtt", account: "a1", topic: "sdr" }, false);
    expect(html).not.toContain('aria-label="Output service"');
    expect(html).toContain(">Service<");
    expect(html).toContain(">MQTT<");
  });

  it("puts the service, its account and its topic on chips", () => {
    const html = render({ service: "mqtt", account: "a1", topic: "sdr" });
    expect(html).toContain('aria-label="Output service"');
    expect(html).toContain(">MQTT<");
    expect(html).toContain('aria-label="MQTT account, from the Accounts list"');
    expect(html).toContain('aria-label="MQTT topic"');
    expect(html).toContain('value="sdr"');
    expect(html).not.toMatch(/<(?:select)/);
  });

  it("shows an account this server does not list as unknown, never as a fault", () => {
    const html = render({ service: "matrix", account: "gone", room_id: "!r:m" });
    expect(html).toContain('aria-label="Matrix account, from the Accounts list"');
    expect(html).toContain(">unknown<");
    expect(html).not.toContain('role="alert"');
  });

  it("keeps no login on the node", () => {
    const html = render({ service: "postgres", account: "", table: "sdrmm_events" });
    expect(html).toContain('aria-label="Created on first write. Lowercase letters, digits and _"');
    expect(html).not.toContain("Password");
    expect(html).not.toContain("Server");
  });

  it("names a notification output", () => {
    const html = render({ service: "desktop" });
    expect(html).toContain(">Notification<");
  });

  it("shows the CSV file name", () => {
    const html = render({ service: "csv", file: "adsb" });
    expect(html).toContain(">CSV file<");
    expect(html).toContain(">File<");
    expect(html).toContain('value="adsb"');
  });

  it("shows the text file name", () => {
    const html = render({ service: "text", file: "pager" });
    expect(html).toContain(">Text file<");
    expect(html).toContain('value="pager"');
  });

  it("offers a message layout where a person reads the result", () => {
    expect(render({ service: "webhook", account: "", format: "discord" })).toContain(">Message<");
    expect(render({ service: "desktop" })).toContain('aria-label="Message layout"');
    expect(render({ service: "csv", file: "adsb" })).not.toContain(">Message<");
  });

  it("keeps a written layout in the settings", () => {
    const node: PatchNode = {
      id: "out",
      kind: "event_output",
      data: { target: { service: "desktop" }, message: "{headline}\n{summary}" },
      position: { x: 0, y: 0 },
    };
    const html = renderFace(EventOutputFace, node, {
      graph: { nodes: [node], edges: [] },
      inspected: true,
    });
    expect(html).toContain(">{headline}\n{summary}</textarea>");
  });

  it("offers the webhook format beside its account", () => {
    const html = render({ service: "webhook", account: "a1", format: "json" });
    expect(html).toContain('aria-label="Webhook account, from the Accounts list"');
    expect(html).toContain('aria-label="Webhook payload format"');
  });
});
