import { describe, expect, it } from "vitest";
import type { DemoSession } from "../../../web/e2e/demoSession";
import CATALOG from "../../../web/src/generated/catalog-fixture.json";
import type { ResolvedState, StateSnapshot, WorkspaceDetail } from "../../../web/src/lib/types";
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

function bodyOf<T>(session: DemoSession, path: RegExp): T {
  const response = session.responses.find(
    (recorded) => recorded.method === "GET" && path.test(recorded.path),
  );
  if (response === undefined) {
    throw new Error(`${session.scene} recorded no ${path.source}`);
  }
  return JSON.parse(response.body) as T;
}

function stateOf(session: DemoSession): StateSnapshot {
  return bodyOf(session, /^\/api\/state$/);
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

  it.each(SCENES.map((scene) => scene.id))("%s serves the current node catalog", (scene) => {
    expect(bodyOf(sessionOf(scene), /^\/api\/patch\/catalog$/)).toEqual(CATALOG);
  });

  it.each(SCENES.map((scene) => scene.id))(
    "%s serves the resolved patch of its workspace",
    (scene) => {
      const session = sessionOf(scene);
      const detail = bodyOf<WorkspaceDetail>(session, /^\/api\/workspaces\/\d+$/);
      const resolved = bodyOf<ResolvedState>(session, /^\/api\/workspaces\/active\/resolved$/);
      expect(resolved.patch.workspace).toBe(detail.id);
      expect(resolved.patch.nodes.map((node) => node.id).toSorted()).toEqual(
        detail.snapshot.graph.nodes.map((node) => node.id).toSorted(),
      );
    },
  );
});
