import type { ComponentType } from "react";
import { afterEach, describe, expect, it } from "vitest";
import { useSkyScanStore } from "../../lib/pointing";
import { useRotatorStore } from "../../lib/rotator";
import type { PatchNode, ResolvedPatch } from "../../lib/types";
import { catalogBody } from "../../test/catalog";
import { renderFace } from "../../test/faceHarness";
import { placed } from "../../test/fixtures";
import { resolvedNodeOf, resolvedPatch, resolvedPort } from "../../test/resolved";
import { ChannelFace } from "./ChannelFace";
import { DecoderLogFace } from "./DecoderLogFace";
import { HuntFace } from "./HuntFace";
import { SWEEP_WANTS, unwiredOf } from "./needs";
import { AudioRecorderFace, BasebandRecorderFace, RecorderFace } from "./RecorderFaces";
import { RotatorFace } from "./RotatorFace";
import { ScannerFace } from "./ScannerFace";
import { ScheduleFace } from "./ScheduleFace";
import { SignalMapFace } from "./SignalMapFace";
import { SkyScanFace } from "./SkyScanFace";
import { TrunkFace } from "./TrunkFace";

const FAULT = 'role="alert"';

function unwired(Face: ComponentType<{ node: PatchNode }>, node: PatchNode): string {
  return renderFace(Face, node, { graph: { nodes: [node], edges: [] }, resolved: null });
}

function sweeper(control: boolean, iq: boolean): ResolvedPatch {
  return resolvedPatch([
    resolvedNodeOf("sweep", "scanner", [
      resolvedPort(
        { name: "control", port_type: "control", direction: "out" },
        { reaches: control ? [{ sink: "ch", port: "control", kinds: [] }] : [] },
      ),
      resolvedPort(
        { name: "iq", port_type: "iq", direction: "in" },
        { feeds: iq ? [{ source: "radio", port: "iq" }] : [] },
      ),
    ]),
  ]);
}

afterEach(() => {
  useRotatorStore.getState().reset();
  useSkyScanStore.getState().reset();
});

describe("unwiredOf", () => {
  it("holds until every input and output has a wire", () => {
    expect(unwiredOf(null, "sweep", SWEEP_WANTS)).toBe(true);
    expect(unwiredOf(sweeper(true, false), "sweep", SWEEP_WANTS)).toBe(true);
    expect(unwiredOf(sweeper(false, true), "sweep", SWEEP_WANTS)).toBe(true);
    expect(unwiredOf(sweeper(true, true), "sweep", SWEEP_WANTS)).toBe(false);
  });
});

describe("a missing wire is never said in words, nor a fault", () => {
  it("rotator: an unwired node hides the server's fault", () => {
    const rotator = placed("rot", {
      kind: "rotator",
      data: { link: { type: "rotctld", address: "localhost:4533" }, mode: "track" },
    });
    useRotatorStore.setState({
      byNode: { rot: { node: "rot", error: "Rotator refused, code 1" } },
    });
    expect(unwired(RotatorFace, rotator)).not.toContain(FAULT);
    const fed = resolvedPatch([
      resolvedNodeOf("rot", "rotator", [
        resolvedPort(
          { name: "control", port_type: "control", direction: "in" },
          { feeds: [{ source: "sat", port: "control" }] },
        ),
      ]),
    ]);
    const wired = renderFace(RotatorFace, rotator, {
      graph: { nodes: [rotator], edges: [] },
      resolved: fed,
    });
    expect(wired).toContain(FAULT);
  });

  it("sky scan: holds Start", () => {
    useSkyScanStore.getState().put({
      node: "sky",
      running: false,
      columns: 1,
      rows: 1,
      cells: [],
      error: "Refused",
    });
    const html = unwired(SkyScanFace, placed("sky", { kind: "sky_scan", data: {} }));
    expect(html).not.toContain(FAULT);
    expect(html).toMatch(/<button[^>]*disabled=""[^>]*>Start</);
  });

  it("trunk: asks for its control channel once fed", () => {
    const trunk = placed("trunk", { kind: "trunk", data: { decoder: "dmr" } });
    expect(unwired(TrunkFace, trunk)).not.toContain("Set the control channel");
    const fed = resolvedPatch([
      resolvedNodeOf("trunk", "trunk", [
        resolvedPort(
          { name: "iq", port_type: "iq", direction: "in" },
          { feeds: [{ source: "radio", port: "iq" }] },
        ),
      ]),
    ]);
    const waiting = renderFace(TrunkFace, trunk, {
      graph: { nodes: [trunk], edges: [] },
      resolved: fed,
    });
    expect(waiting).toContain("Set the control channel");
    expect(waiting).not.toContain("text-warn");
  });

  it("every face: no wire hint, no fault", () => {
    const cases: [ComponentType<{ node: PatchNode }>, PatchNode][] = [
      [ScannerFace, placed("scan", { kind: "scanner", data: {} })],
      [HuntFace, placed("hunt", { kind: "hunt", data: {} })],
      [SignalMapFace, placed("map", catalogBody("signal_map"))],
      [ChannelFace, placed("ch", catalogBody("channel"))],
      [RecorderFace, placed("rec", catalogBody("recorder"))],
      [AudioRecorderFace, placed("wav", catalogBody("audio_recorder"))],
      [BasebandRecorderFace, placed("bb", catalogBody("baseband_recorder"))],
      [ScheduleFace, placed("clock", catalogBody("schedule"))],
      [DecoderLogFace, placed("log", catalogBody("decoder_log"))],
    ];
    for (const [Face, node] of cases) {
      const html = unwired(Face, node);
      expect(html, node.kind).not.toMatch(/>Wire [^<]*</);
      expect(html, node.kind).not.toContain(FAULT);
    }
  });
});
