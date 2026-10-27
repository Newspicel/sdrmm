import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it } from "vitest";
import { DEVICES_KEY } from "../lib/api";
import type { DeviceInfo, DevicesResponse } from "../lib/types";
import { DeviceChoices } from "./OpenRadio";

function render(response: DevicesResponse): string {
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  client.setQueryData(DEVICES_KEY, response);
  return renderToStaticMarkup(
    <QueryClientProvider client={client}>
      <DeviceChoices onChoose={() => undefined} onAddNetwork={() => undefined} />
    </QueryClientProvider>,
  );
}

const VIRTUAL: DeviceInfo = { driver: "virtual", key: "band", label: "Test band (virtual)" };

describe("DeviceChoices", () => {
  it("offers the virtual radios the server says it serves", () => {
    expect(render({ devices: [VIRTUAL], synthetic: true })).toContain("Virtual (1)");
  });

  it("counts none when the server does not serve them", () => {
    const html = render({ devices: [VIRTUAL], synthetic: false });
    expect(html).toContain("Virtual");
    expect(html).not.toContain("Virtual (1)");
  });
});
