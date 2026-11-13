import type { Node, NodeProps } from "@xyflow/react";
import { type ComponentType, type CSSProperties, lazy, Suspense } from "react";
import type { NodeKind, PatchNode } from "../../lib/types";
import type { FlowData } from "../Canvas";
import { nodeMinSize, nodeSize } from "../graph";
import { CanvasSurface } from "./NodeShell";

type Face = ComponentType<{ node: PatchNode }>;

/// A face whose code loads the first time a node of its kind is drawn, so startup carries none.
function lazyFace<K extends string>(load: () => Promise<Record<K, Face>>, name: K): Face {
  const Loaded = lazy<Face>(() => load().then((module) => ({ default: module[name] })));
  return function LazyFace({ node }: { node: PatchNode }) {
    return (
      <Suspense fallback={null}>
        <Loaded node={node} />
      </Suspense>
    );
  };
}

function mount(Face: Face) {
  return function FaceNode({ data }: NodeProps<Node<FlowData>>) {
    return (
      <CanvasSurface>
        <Face node={data.node} />
      </CanvasSurface>
    );
  };
}

const ProcessorFace = lazyFace(() => import("./ArrayProcessorFace"), "ArrayProcessorFace");

/** Kinds with a face of their own. Every other kind is an array processor, drawn from its type. */
const FACES: Partial<Record<NodeKind, Face>> = {
  device: lazyFace(() => import("./DeviceFace"), "DeviceFace"),
  gps: lazyFace(() => import("./GpsFace"), "GpsFace"),
  channel: lazyFace(() => import("./ChannelFace"), "ChannelFace"),
  event_output: lazyFace(() => import("./EventOutputFace"), "EventOutputFace"),
  scope: lazyFace(() => import("./ScopeFace"), "ScopeFace"),
  baseband_scope: lazyFace(() => import("./BasebandScopeFace"), "BasebandScopeFace"),
  speaker: lazyFace(() => import("./SpeakerFace"), "SpeakerFace"),
  map: lazyFace(() => import("./MapFace"), "MapFace"),
  signal_map: lazyFace(() => import("./SignalMapFace"), "SignalMapFace"),
  propagation: lazyFace(() => import("./PropagationFace"), "PropagationFace"),
  readout: lazyFace(() => import("./ReadoutFace"), "ReadoutFace"),
  decoder_log: lazyFace(() => import("./DecoderLogFace"), "DecoderLogFace"),
  trunk: lazyFace(() => import("./TrunkFace"), "TrunkFace"),
  spectrum_monitor: lazyFace(() => import("./SpectrumMonitorFace"), "SpectrumMonitorFace"),
  event_filter: lazyFace(() => import("./EventFilterFace"), "EventFilterFace"),
  audio_fx: lazyFace(() => import("./AudioFxFace"), "AudioFxFace"),
  video: lazyFace(() => import("./VideoFace"), "VideoFace"),
  recorder: lazyFace(() => import("./RecorderFaces"), "RecorderFace"),
  audio_recorder: lazyFace(() => import("./RecorderFaces"), "AudioRecorderFace"),
  baseband_recorder: lazyFace(() => import("./RecorderFaces"), "BasebandRecorderFace"),
  network_export: lazyFace(() => import("./NetworkExportFace"), "NetworkExportFace"),
  export: lazyFace(() => import("./ExportFace"), "ExportFace"),
  scanner: lazyFace(() => import("./ScannerFace"), "ScannerFace"),
  hunt: lazyFace(() => import("./HuntFace"), "HuntFace"),
  schedule: lazyFace(() => import("./ScheduleFace"), "ScheduleFace"),
  satellite: lazyFace(() => import("./SatelliteFace"), "SatelliteFace"),
  rotator: lazyFace(() => import("./RotatorFace"), "RotatorFace"),
  triangulation: lazyFace(() => import("./TriangulationFace"), "TriangulationFace"),
  bearing_fix: lazyFace(() => import("./BearingFixFace"), "BearingFixFace"),
  array: lazyFace(() => import("./ArrayFace"), "ArrayFace"),
  microphone: lazyFace(() => import("./MicrophoneFace"), "MicrophoneFace"),
  modulator: lazyFace(() => import("./ModulatorFace"), "ModulatorFace"),
  transmit: lazyFace(() => import("./TransmitFace"), "TransmitFace"),
  note: lazyFace(() => import("./NoteFace"), "NoteFace"),
};

const PROCESSOR_FLOW = "processor";

/** A node's face: its kind's own, or the one every array processor shares. */
export function NodeFace({ node }: { node: PatchNode }) {
  const Face = FACES[node.kind];
  return Face === undefined ? <ProcessorFace node={node} /> : <Face node={node} />;
}

/** The React Flow type that draws a node of `kind`. */
export function flowTypeOf(kind: NodeKind): string {
  return kind in FACES ? kind : PROCESSOR_FLOW;
}

export const NODE_TYPES: Record<string, ComponentType<NodeProps<Node<FlowData>>>> = {
  ...Object.fromEntries(Object.entries(FACES).map(([kind, Face]) => [kind, mount(Face)])),
  [PROCESSOR_FLOW]: mount(ProcessorFace),
};

export function faceSize(node: PatchNode): CSSProperties {
  const size = nodeSize(node.kind);
  return { width: size.h === undefined ? size.w : "100%", height: "100%" };
}

/// A rack slot may be narrower than a face can draw; the slot scrolls rather than squeezing it.
export function rackFaceSize(node: PatchNode): CSSProperties {
  return { minWidth: nodeMinSize(node.kind, []).w };
}
