const UTF8 = new TextEncoder();

function hexBytes(bytes: Uint8Array): string {
  return Array.from(bytes, (byte) => byte.toString(16).padStart(2, "0")).join("");
}

export function hexUtf8(text: string): string {
  return hexBytes(UTF8.encode(text));
}

export function randomHex(byteCount: number): string {
  return hexBytes(crypto.getRandomValues(new Uint8Array(byteCount)));
}
