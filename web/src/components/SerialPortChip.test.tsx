import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it } from "vitest";
import { serialPortsQuery } from "../lib/api";
import type { Form, Settings } from "../lib/settingsForm";
import { SettingsForm } from "./SettingsForm";

const FORM: Form = [
  { key: "vocoder", label: "Vocoder", control: { type: "serial_port", empty: "blip25" } },
];

function render(settings: Settings): string {
  const client = new QueryClient();
  client.setQueryData(serialPortsQuery().queryKey, {
    devices: [{ path: "/dev/cu.usbserial-DVSI1", product: "ThumbDV" }],
  });
  return renderToStaticMarkup(
    <QueryClientProvider client={client}>
      <SettingsForm form={FORM} settings={settings} choices={[]} onSettings={() => {}} />
    </QueryClientProvider>,
  );
}

describe("SerialPortChip", () => {
  it("names software synthesis while no port is picked", () => {
    expect(render({})).toContain("blip25");
  });

  it("shows the picked port by its name", () => {
    expect(render({ vocoder: "/dev/cu.usbserial-DVSI1" })).toContain("cu.usbserial-DVSI1");
  });

  it("keeps a port that is not plugged in", () => {
    expect(render({ vocoder: "/dev/ttyUSB7" })).toContain("ttyUSB7");
  });
});
