import { FrameChunker, MICROPHONE_RATE_HZ, peakOf } from "./microphone";

const PROCESSOR_NAME = "sdr-microphone-capture";

const processorSource = `
"use strict";
class CaptureProcessor extends AudioWorkletProcessor {
  process(inputs) {
    const channel = inputs[0] && inputs[0][0];
    if (channel && channel.length > 0) {
      const block = channel.slice();
      this.port.postMessage(block, [block.buffer]);
    }
    return true;
  }
}
registerProcessor(${JSON.stringify(PROCESSOR_NAME)}, CaptureProcessor);
`;

export interface Capture {
  stop(): void;
}

/** Opens the microphone and hands back 20 ms frames at the uplink rate until stopped. */
export async function startCapture(
  onFrame: (frame: Float32Array, peak: number) => void,
): Promise<Capture> {
  const stream = await navigator.mediaDevices.getUserMedia({
    audio: { channelCount: 1, echoCancellation: false, noiseSuppression: false },
  });
  const context = new AudioContext({ sampleRate: MICROPHONE_RATE_HZ });
  const url = URL.createObjectURL(new Blob([processorSource], { type: "text/javascript" }));
  const release = (): void => {
    for (const track of stream.getTracks()) {
      track.stop();
    }
    URL.revokeObjectURL(url);
    void context.close();
  };
  try {
    await context.audioWorklet.addModule(url);
  } catch (error) {
    release();
    throw error;
  }
  const chunker = new FrameChunker(context.sampleRate, (frame) => onFrame(frame, peakOf(frame)));
  const source = context.createMediaStreamSource(stream);
  const tap = new AudioWorkletNode(context, PROCESSOR_NAME, {
    numberOfInputs: 1,
    numberOfOutputs: 0,
    channelCount: 1,
  });
  tap.port.onmessage = (event: MessageEvent<Float32Array>) => chunker.push(event.data);
  source.connect(tap);
  return {
    stop() {
      tap.port.onmessage = null;
      source.disconnect();
      release();
    },
  };
}
