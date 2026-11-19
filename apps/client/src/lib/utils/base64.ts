/** Bytes converted per `String.fromCharCode` call, below engine argument limits. */
const BASE64_CHUNK_BYTES = 0x8000;

/**
 * Encodes bytes as standard padded base64, for IPC payloads that must travel as JSON text.
 *
 * @param bytes The bytes to encode.
 * @returns The base64 text.
 */
export function encodeBase64(bytes: Uint8Array): string {
  const parts: string[] = [];
  for (let start = 0; start < bytes.length; start += BASE64_CHUNK_BYTES) {
    parts.push(String.fromCharCode(...bytes.subarray(start, start + BASE64_CHUNK_BYTES)));
  }
  return btoa(parts.join(""));
}
