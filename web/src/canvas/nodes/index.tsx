import type { Node, NodeProps } from "@xyflow/react";
import type { ComponentType, CSSProperties } from "react";
import type { NodeKind, PatchNode } from "../../lib/types";
import type { FlowData } from "../Canvas";
import { nodeMinSize, nodeSize } from "../graph";
import { ArrayFace } from "./ArrayFace";
import { AudioFxFace } from "./AudioFxFace";
import { BasebandScopeFace } from "./BasebandScopeFace";
import { BeamformerFace } from "./BeamformerFace";
import { BearingFixFace } from "./BearingFixFace";
import { ChannelFace } from "./ChannelFace";
import { CorrelatorFace } from "./CorrelatorFace";
import { DecoderLogFace } from "./DecoderLogFace";
import { DeviceFace } from "./DeviceFace";
import { DfFace } from "./DfFace";
import { EventFilterFace } from "./EventFilterFace";
import { EventOutputFace } from "./EventOutputFace";
import { ExportFace } from "./ExportFace";
import { GpsFace } from "./GpsFace";
import { HuntFace } from "./HuntFace";
import { MapFace } from "./MapFace";
import { MicrophoneFace } from "./MicrophoneFace";
import { ModulatorFace } from "./ModulatorFace";
import { NetworkExportFace } from "./NetworkExportFace";
import { CanvasSurface } from "./NodeShell";
import { NoteFace } from "./NoteFace";
import { PluginProcessorFace } from "./PluginProcessorFace";
import { PolarimeterFace } from "./PolarimeterFace";
import { PropagationFace } from "./PropagationFace";
import { RadarFace } from "./RadarFace";
import { ReadoutFace } from "./ReadoutFace";
import { AudioRecorderFace, BasebandRecorderFace, RecorderFace } from "./RecorderFaces";
import { RotatorFace } from "./RotatorFace";
import { SatelliteFace } from "./SatelliteFace";
import { ScannerFace } from "./ScannerFace";
import { ScheduleFace } from "./ScheduleFace";
import { ScopeFace } from "./ScopeFace";
import { SignalMapFace } from "./SignalMapFace";
import { SpatialSpectrumFace } from "./SpatialSpectrumFace";
import { SpeakerFace } from "./SpeakerFace";
import { SpectrumMonitorFace } from "./SpectrumMonitorFace";
import { StitchFace } from "./StitchFace";
import { TransmitFace } from "./TransmitFace";
import { TriangulationFace } from "./TriangulationFace";
import { TrunkFace } from "./TrunkFace";
import { VideoFace } from "./VideoFace";

type Face = ComponentType<{ node: PatchNode }>;

function mount(Face: Face) {
  return function FaceNode({ data }: NodeProps<Node<FlowData>>) {
    return (
      <CanvasSurface>
        <Face node={data.node} />
      </CanvasSurface>
    );
  };
}

export const NODE_TYPES: Record<NodeKind, ComponentType<NodeProps<Node<FlowData>>>> = {
  device: mount(DeviceFace),
  gps: mount(GpsFace),
  channel: mount(ChannelFace),
  event_output: mount(EventOutputFace),
  scope: mount(ScopeFace),
  baseband_scope: mount(BasebandScopeFace),
  speaker: mount(SpeakerFace),
  map: mount(MapFace),
  signal_map: mount(SignalMapFace),
  propagation: mount(PropagationFace),
  readout: mount(ReadoutFace),
  decoder_log: mount(DecoderLogFace),
  trunk: mount(TrunkFace),
  spectrum_monitor: mount(SpectrumMonitorFace),
  event_filter: mount(EventFilterFace),
  audio_fx: mount(AudioFxFace),
  video: mount(VideoFace),
  recorder: mount(RecorderFace),
  audio_recorder: mount(AudioRecorderFace),
  baseband_recorder: mount(BasebandRecorderFace),
  network_export: mount(NetworkExportFace),
  export: mount(ExportFace),
  scanner: mount(ScannerFace),
  hunt: mount(HuntFace),
  schedule: mount(ScheduleFace),
  satellite: mount(SatelliteFace),
  rotator: mount(RotatorFace),
  triangulation: mount(TriangulationFace),
  bearing_fix: mount(BearingFixFace),
  array: mount(ArrayFace),
  df: mount(DfFace),
  beamformer: mount(BeamformerFace),
  passive_radar: mount(RadarFace),
  stitch: mount(StitchFace),
  spatial_spectrum: mount(SpatialSpectrumFace),
  correlator: mount(CorrelatorFace),
  polarimeter: mount(PolarimeterFace),
  processor: mount(PluginProcessorFace),
  microphone: mount(MicrophoneFace),
  modulator: mount(ModulatorFace),
  transmit: mount(TransmitFace),
  note: mount(NoteFace),
};

export const FACES: Record<NodeKind, Face> = {
  device: DeviceFace,
  gps: GpsFace,
  channel: ChannelFace,
  event_output: EventOutputFace,
  scope: ScopeFace,
  baseband_scope: BasebandScopeFace,
  speaker: SpeakerFace,
  map: MapFace,
  signal_map: SignalMapFace,
  propagation: PropagationFace,
  readout: ReadoutFace,
  decoder_log: DecoderLogFace,
  trunk: TrunkFace,
  spectrum_monitor: SpectrumMonitorFace,
  event_filter: EventFilterFace,
  audio_fx: AudioFxFace,
  video: VideoFace,
  recorder: RecorderFace,
  audio_recorder: AudioRecorderFace,
  baseband_recorder: BasebandRecorderFace,
  network_export: NetworkExportFace,
  export: ExportFace,
  scanner: ScannerFace,
  hunt: HuntFace,
  schedule: ScheduleFace,
  satellite: SatelliteFace,
  rotator: RotatorFace,
  triangulation: TriangulationFace,
  bearing_fix: BearingFixFace,
  array: ArrayFace,
  df: DfFace,
  beamformer: BeamformerFace,
  passive_radar: RadarFace,
  stitch: StitchFace,
  spatial_spectrum: SpatialSpectrumFace,
  correlator: CorrelatorFace,
  polarimeter: PolarimeterFace,
  processor: PluginProcessorFace,
  microphone: MicrophoneFace,
  modulator: ModulatorFace,
  transmit: TransmitFace,
  note: NoteFace,
};

export function faceSize(node: PatchNode): CSSProperties {
  const size = nodeSize(node.kind);
  return { width: size.h === undefined ? size.w : "100%", height: "100%" };
}

/// A rack slot may be narrower than a face can draw; the slot scrolls rather than squeezing it.
export function rackFaceSize(node: PatchNode): CSSProperties {
  return { minWidth: nodeMinSize(node.kind, []).w };
}
