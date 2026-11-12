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

export function unhex(text: string): Uint8Array<ArrayBuffer> | null {
  if (text.length % 2 !== 0 || !/^[0-9a-f]*$/i.test(text)) {
    return null;
  }
  const bytes = new Uint8Array(text.length / 2);
  for (let i = 0; i < bytes.length; i++) {
    bytes[i] = Number.parseInt(text.slice(i * 2, i * 2 + 2), 16);
  }
  return bytes;
}
