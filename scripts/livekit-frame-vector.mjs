// Mirrors livekit client-sdk-js FrameCryptor for an Opus frame:
// src/e2ee/constants.ts (SALT, IV_LENGTH, UNENCRYPTED_BYTES.audio),
// src/e2ee/utils.ts (deriveKeys: HKDF SHA-256, info of 128 zero bytes),
// src/e2ee/worker/FrameCryptor.ts (makeIV, encodeFunction, frame layout).
const SALT = "LKFrameEncryptionKey";
const IV_LENGTH = 12;
const UNENCRYPTED_AUDIO = 1;

const hex = (bytes) => Buffer.from(bytes).toString("hex");
const subtle = globalThis.crypto.subtle;

const material = Uint8Array.from({ length: 32 }, (_, i) => i + 1);
const ssrc = 0x01020304;
const timestamp = 0x00112233;
const sendCount = 7;
const keyIndex = 3;
const opus = Uint8Array.from({ length: 40 }, (_, i) => (i * 7 + 1) & 0xff);

const hkdfKey = await subtle.importKey("raw", material, "HKDF", false, ["deriveKey"]);
const key = await subtle.deriveKey(
  { name: "HKDF", salt: new TextEncoder().encode(SALT), hash: "SHA-256", info: new ArrayBuffer(128) },
  hkdfKey,
  { name: "AES-GCM", length: 128 },
  true,
  ["encrypt", "decrypt"],
);
const keyBytes = new Uint8Array(await subtle.exportKey("raw", key));

const iv = new DataView(new ArrayBuffer(IV_LENGTH));
iv.setUint32(0, ssrc);
iv.setUint32(4, timestamp);
iv.setUint32(8, (timestamp - (sendCount % 0xffff)) >>> 0);

const header = opus.slice(0, UNENCRYPTED_AUDIO);
const cipher = new Uint8Array(
  await subtle.encrypt(
    { name: "AES-GCM", iv: iv.buffer, additionalData: header },
    key,
    opus.slice(UNENCRYPTED_AUDIO),
  ),
);
const frame = new Uint8Array([...header, ...cipher, ...new Uint8Array(iv.buffer), IV_LENGTH, keyIndex]);

console.log("MATERIAL", hex(material));
console.log("KEY", hex(keyBytes));
console.log("OPUS", hex(opus));
console.log("FRAME", hex(frame));
