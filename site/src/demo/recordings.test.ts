import { describe, expect, it } from "vitest";
import type { DemoSession } from "../../../web/e2e/demoSession";
import type { StateSnapshot } from "../../../web/src/lib/types";
import { SCENES } from "./scenes";

const SESSIONS = import.meta.glob<DemoSession>("../../public/demo/*.json", {
  eager: true,
  import: "default",
});

function sessionOf(scene: string): DemoSession {
  const session = SESSIONS[`../../public/demo/${scene}.json`];
  if (session === undefined) {
    throw new Error(`no recording for ${scene}`);
  }
  return session;
}

function stateOf(session: DemoSession): StateSnapshot {
  const response = session.responses.find(
    (recorded) => recorded.method === "GET" && recorded.path === "/api/state",
  );
  if (response === undefined) {
    throw new Error(`${session.scene} recorded no state`);
  }
  return JSON.parse(response.body) as StateSnapshot;
}

describe("demo recordings", () => {
  it.each(SCENES.map((scene) => scene.id))("%s carries the current band miss shape", (scene) => {
    const misses = stateOf(sessionOf(scene)).device_sets.flatMap((set) =>
      set.channels.map((channel) => channel.out_of_band),
    );
    for (const miss of misses) {
      expect(miss === undefined || miss === null || typeof miss.reason === "string").toBe(true);
    }
  });
});
