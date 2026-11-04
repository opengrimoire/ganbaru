import { createHash } from "node:crypto";
import { readFile, writeFile } from "node:fs/promises";
import { fileURLToPath } from "node:url";
import path from "node:path";

/** Command-line flag that lets a generator record the digest of a source URL it has not pinned yet. */
export const PIN_NEW_SOURCE_FLAG = "--pin-new-source";

/** Largest upstream source a generator downloads. */
export const MAX_PINNED_SOURCE_BYTES = 16 * 1024 * 1024;

const defaultPinsPath = path.join(path.dirname(fileURLToPath(import.meta.url)), "pinned-sources.json");
const digestPattern = /^[0-9a-f]{64}$/u;

/**
 * Returns the lowercase hex SHA-256 digest of downloaded source bytes.
 *
 * @param {Uint8Array} bytes Downloaded bytes.
 * @returns {string} Hex digest.
 */
export function sourceDigest(bytes) {
  return createHash("sha256").update(bytes).digest("hex");
}

/**
 * Validates the pin record: an object mapping HTTPS source URLs to SHA-256 hex digests.
 *
 * @param {unknown} value Parsed pin file contents.
 * @returns {Map<string, string>} Pins keyed by URL.
 */
export function parsePins(value) {
  if (!value || typeof value !== "object" || Array.isArray(value)) {
    throw new Error("Pinned sources must be an object keyed by source URL");
  }
  const pins = new Map();
  for (const [url, digest] of Object.entries(value)) {
    if (!url.startsWith("https://")) throw new Error(`Pinned source ${url} is not an HTTPS URL`);
    if (typeof digest !== "string" || !digestPattern.test(digest)) {
      throw new Error(`Pinned source ${url} has a malformed SHA-256 digest`);
    }
    pins.set(url, digest);
  }
  return pins;
}

/**
 * Decides whether downloaded bytes may be used. A pinned URL must match its digest exactly; an
 * unpinned URL is accepted only when the maintainer asked to pin it, so a new upstream version is
 * a deliberate step whose generated diff gets reviewed.
 *
 * @param {Map<string, string>} pins Recorded pins.
 * @param {string} url Source URL.
 * @param {string} digest Digest of the downloaded bytes.
 * @param {boolean} allowNewPin Whether an unpinned URL may be recorded.
 * @returns {"pinned" | "new"} Whether the source matched a pin or should be recorded.
 */
export function checkPin(pins, url, digest, allowNewPin) {
  const pinned = pins.get(url);
  if (pinned === digest) return "pinned";
  if (pinned !== undefined) {
    throw new Error(`${url} changed upstream: expected SHA-256 ${pinned}, received ${digest}`);
  }
  if (!allowNewPin) {
    throw new Error(`${url} is not pinned; rerun with ${PIN_NEW_SOURCE_FLAG} and review the generated diff`);
  }
  return "new";
}

/**
 * Formats pins as stable JSON sorted by URL.
 *
 * @param {Map<string, string>} pins Pins keyed by URL.
 * @returns {string} File contents.
 */
export function formatPins(pins) {
  const sorted = Object.fromEntries([...pins].sort(([left], [right]) => left.localeCompare(right)));
  return `${JSON.stringify(sorted, null, 2)}\n`;
}

/**
 * Downloads a generator source and checks it against its recorded SHA-256 pin.
 *
 * @param {string} url HTTPS source URL that names an exact upstream version.
 * @param {{ allowNewPin: boolean, accept?: string, pinsPath?: string }} options Pinning options.
 * @returns {Promise<string>} UTF-8 source text.
 */
export async function fetchPinnedSource(url, { allowNewPin, accept, pinsPath = defaultPinsPath }) {
  const pins = parsePins(JSON.parse(await readFile(pinsPath, "utf8")));
  const response = await fetch(url, { redirect: "error", headers: accept ? { accept } : {} });
  if (!response.ok) throw new Error(`Could not download ${url}: HTTP ${response.status}`);
  const bytes = new Uint8Array(await response.arrayBuffer());
  if (bytes.byteLength > MAX_PINNED_SOURCE_BYTES) {
    throw new Error(`${url} exceeds ${MAX_PINNED_SOURCE_BYTES} bytes`);
  }
  const digest = sourceDigest(bytes);
  if (checkPin(pins, url, digest, allowNewPin) === "new") {
    pins.set(url, digest);
    await writeFile(pinsPath, formatPins(pins), "utf8");
    process.stdout.write(`Pinned ${url} at SHA-256 ${digest}.\n`);
  }
  return new TextDecoder("utf-8", { fatal: true }).decode(bytes);
}
