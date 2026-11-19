import { expect, test } from "@playwright/test";
import type { WorkspaceSnapshot } from "../src/lib/types";

test("persists all DAB transmission modes separately from audio generation", async ({ page }) => {
  const snapshot: WorkspaceSnapshot = {
    version: 4,
    graph: {
      nodes: [
        { id: "dab", kind: "channel", position: { x: 0, y: 0 }, data: { channel_type: "dab" } },
      ],
      edges: [],
    },
  };
  const desk = await page.request.get("/api/workspaces").then((response) => response.json());
  const created = await page.request.post("/api/workspaces", {
    data: { name: "DAB modes", snapshot },
  });
  expect(created.ok()).toBe(true);
  const { id } = await created.json();
  expect((await page.request.post(`/api/workspaces/${id}/activate`, { data: {} })).ok()).toBe(true);
  try {
    await page.goto("/");
    const node = page.locator('.react-flow__node[data-id="dab"]');
    const inspector = page.getByRole("complementary", { name: "Inspector" });
    await node.locator("header").click();
    const chip = (title: string) => inspector.getByRole("combobox", { name: title, exact: true });
    const pick = async (title: string, option: string): Promise<void> => {
      await chip(title).click();
      await page.getByRole("option", { name: option, exact: true }).click();
    };
    await pick("DAB generation", "DAB+");
    for (const mode of ["II", "III", "IV", "I"]) {
      await pick("DAB transmission mode", mode);
      await expect(chip("DAB transmission mode")).toHaveText(mode);
      await expect
        .poll(async () => {
          const detail = await page.request
            .get(`/api/workspaces/${id}`)
            .then((response) => response.json());
          return detail.state.channels.find((channel: { node: string }) => channel.node === "dab")
            ?.settings.params.settings;
        })
        .toMatchObject({ mode: "dab_plus", transmission_mode: mode.toLowerCase() });
    }
    await page.reload();
    await node.locator("header").click();
    await expect(chip("DAB transmission mode")).toHaveText("I");
    await expect(chip("DAB generation")).toHaveText("DAB+");
  } finally {
    await page.request.post(`/api/workspaces/${desk.active}/activate`, { data: {} });
    await page.request.delete(`/api/workspaces/${id}`);
  }
});
