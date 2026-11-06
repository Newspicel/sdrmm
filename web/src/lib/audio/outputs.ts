export interface AudioOutput {
  readonly id: string;
  readonly name: string;
}

export interface AudioOutputs {
  readonly outputs: readonly AudioOutput[];
  /// False while the browser hides device names until the page may use the microphone.
  readonly named: boolean;
}

const SYSTEM_ALIASES = new Set(["", "default", "communications"]);

export function outputsSupported(): boolean {
  return typeof AudioContext !== "undefined" && "setSinkId" in AudioContext.prototype;
}

export async function listOutputs(): Promise<AudioOutputs> {
  const devices = globalThis.navigator?.mediaDevices;
  if (devices === undefined) {
    return { outputs: [], named: true };
  }
  const found = (await devices.enumerateDevices()).filter(
    (device) => device.kind === "audiooutput" && !SYSTEM_ALIASES.has(device.deviceId),
  );
  return {
    outputs: found
      .filter((device) => device.label !== "")
      .map((device) => ({ id: device.deviceId, name: device.label })),
    named: found.every((device) => device.label !== ""),
  };
}

export async function outputIdOf(name: string): Promise<string> {
  if (!outputsSupported()) {
    throw new Error("This browser plays only on the system output");
  }
  const { outputs } = await listOutputs();
  const output = outputs.find((candidate) => candidate.name === name);
  if (output === undefined) {
    throw new Error(`Output "${name}" not found`);
  }
  return output.id;
}

/// A browser names output devices only once the page may use the microphone; asking once and
/// closing the stream at once is the only way a page can earn that.
export async function revealOutputNames(): Promise<void> {
  const stream = await navigator.mediaDevices.getUserMedia({ audio: true });
  for (const track of stream.getTracks()) {
    track.stop();
  }
}

export function onOutputsChanged(listener: () => void): () => void {
  const devices = globalThis.navigator?.mediaDevices;
  devices?.addEventListener("devicechange", listener);
  return () => devices?.removeEventListener("devicechange", listener);
}
