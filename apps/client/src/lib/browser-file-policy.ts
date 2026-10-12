const BYTES_PER_KIBIBYTE = 1024;
const KIBIBYTES_PER_MEBIBYTE = 1024;
const MANAGED_IMAGE_HEADER_MAX_BYTES = 256 * BYTES_PER_KIBIBYTE;

/** MIME types accepted by managed icon and page-cover uploads. */
export const MANAGED_IMAGE_FILE_ACCEPT = "image/png,image/jpeg,image/webp";

/** Maximum compressed icon image size shared with the Rust asset boundary. */
export const MANAGED_ICON_IMAGE_MAX_MEGABYTES: number = 3;
export const MANAGED_ICON_IMAGE_MAX_BYTES: number = MANAGED_ICON_IMAGE_MAX_MEGABYTES
  * KIBIBYTES_PER_MEBIBYTE
  * BYTES_PER_KIBIBYTE;

/** Maximum compressed Notes cover image size shared with the Rust asset boundary. */
export const NOTES_PAGE_COVER_IMAGE_MAX_MEGABYTES: number = 8;
export const NOTES_PAGE_COVER_IMAGE_MAX_BYTES: number = NOTES_PAGE_COVER_IMAGE_MAX_MEGABYTES
  * KIBIBYTES_PER_MEBIBYTE
  * BYTES_PER_KIBIBYTE;

/** Maximum decoded geometry shared with `ganbaru_notes::assets::image_metadata`. */
export const MANAGED_IMAGE_MAX_DIMENSION_PIXELS: number = 8192;
export const MANAGED_IMAGE_MAX_TOTAL_PIXELS: number = 16_000_000;
export const MANAGED_IMAGE_MAX_MEGAPIXELS: number = MANAGED_IMAGE_MAX_TOTAL_PIXELS / 1_000_000;

/** Maximum Notes database CSV size shared with the Rust import boundary. */
export const NOTES_DATABASE_CSV_MAX_KIBIBYTES = 512;
export const NOTES_DATABASE_CSV_MAX_BYTES = NOTES_DATABASE_CSV_MAX_KIBIBYTES
  * BYTES_PER_KIBIBYTE;

const MANAGED_IMAGE_MIME_TYPES = new Set(MANAGED_IMAGE_FILE_ACCEPT.split(","));
const PNG_SIGNATURE = [0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a] as const;

/** Managed image encoding identified from its binary header. */
export type ManagedImageKind = "png" | "jpeg" | "webp";

/** Canonical MIME type derived from a managed image's binary header. */
export type ManagedImageMimeType = "image/png" | "image/jpeg" | "image/webp";

/** Dimensions and encoding read without decoding image pixels. */
export interface ManagedImageMetadata {
  readonly kind: ManagedImageKind;
  readonly mimeType: ManagedImageMimeType;
  readonly width: number;
  readonly height: number;
}

/** Result of inspecting a bounded managed-image header. */
export type ManagedImageHeaderResult =
  | { readonly ok: true; readonly metadata: ManagedImageMetadata }
  | { readonly ok: false; readonly issue: "unsupported-signature" | "malformed-header" };

/** Reason a browser-selected image cannot enter the managed asset boundary. */
export type ManagedImageFileIssue =
  | "empty"
  | "unsupported-type"
  | "too-large"
  | "invalid-image"
  | "dimensions-too-large"
  | "too-many-pixels";

/** Result of bounded browser-file inspection before a full data URL allocation. */
export type ManagedImageFileInspection =
  | { readonly ok: true; readonly metadata: ManagedImageMetadata }
  | { readonly ok: false; readonly issue: ManagedImageFileIssue };

/** File metadata available before reading user-selected browser content. */
export interface BrowserFileMetadata {
  readonly size: number;
  readonly type: string;
}

/** Minimal browser file contract needed for bounded header inspection. */
export interface BrowserImageFile extends BrowserFileMetadata {
  slice(start?: number, end?: number): Blob;
}

function matchesBytes(bytes: Uint8Array, offset: number, expected: readonly number[]): boolean {
  if (offset + expected.length > bytes.length) return false;
  return expected.every((value, index) => bytes[offset + index] === value);
}

function readU16Be(bytes: Uint8Array, offset: number): number | null {
  if (offset + 2 > bytes.length) return null;
  return (bytes[offset] << 8) | bytes[offset + 1];
}

function readU16Le(bytes: Uint8Array, offset: number): number | null {
  if (offset + 2 > bytes.length) return null;
  return bytes[offset] | (bytes[offset + 1] << 8);
}

function readU24Le(bytes: Uint8Array, offset: number): number | null {
  if (offset + 3 > bytes.length) return null;
  return bytes[offset] | (bytes[offset + 1] << 8) | (bytes[offset + 2] << 16);
}

function readU32Be(bytes: Uint8Array, offset: number): number | null {
  if (offset + 4 > bytes.length) return null;
  return (
    (bytes[offset] * 0x1000000)
    + (bytes[offset + 1] << 16)
    + (bytes[offset + 2] << 8)
    + bytes[offset + 3]
  );
}

function readU32Le(bytes: Uint8Array, offset: number): number | null {
  if (offset + 4 > bytes.length) return null;
  return (
    bytes[offset]
    + (bytes[offset + 1] << 8)
    + (bytes[offset + 2] << 16)
    + (bytes[offset + 3] * 0x1000000)
  );
}

function validMetadata(
  kind: ManagedImageKind,
  mimeType: ManagedImageMimeType,
  width: number | null,
  height: number | null,
): ManagedImageHeaderResult {
  if (width === null || height === null || width <= 0 || height <= 0) {
    return { ok: false, issue: "malformed-header" };
  }
  return { ok: true, metadata: { kind, mimeType, width, height } };
}

function parsePngMetadata(bytes: Uint8Array): ManagedImageHeaderResult {
  if (bytes.length < 33) return { ok: false, issue: "malformed-header" };
  if (readU32Be(bytes, 8) !== 13 || !matchesBytes(bytes, 12, [0x49, 0x48, 0x44, 0x52])) {
    return { ok: false, issue: "malformed-header" };
  }
  return validMetadata("png", "image/png", readU32Be(bytes, 16), readU32Be(bytes, 20));
}

function isJpegSofMarker(marker: number): boolean {
  return (
    (marker >= 0xc0 && marker <= 0xc3)
    || (marker >= 0xc5 && marker <= 0xc7)
    || (marker >= 0xc9 && marker <= 0xcb)
    || (marker >= 0xcd && marker <= 0xcf)
  );
}

function parseJpegMetadata(bytes: Uint8Array): ManagedImageHeaderResult {
  let offset = 2;
  while (offset < bytes.length) {
    if (bytes[offset] !== 0xff) return { ok: false, issue: "malformed-header" };
    while (offset < bytes.length && bytes[offset] === 0xff) offset += 1;
    if (offset >= bytes.length) return { ok: false, issue: "malformed-header" };
    const marker = bytes[offset];
    offset += 1;

    if (marker === 0x00 || marker === 0xd8 || marker === 0xd9 || marker === 0xda) {
      return { ok: false, issue: "malformed-header" };
    }
    if (marker === 0x01 || (marker >= 0xd0 && marker <= 0xd7)) continue;

    const segmentLength = readU16Be(bytes, offset);
    if (segmentLength === null || segmentLength < 2 || offset + segmentLength > bytes.length) {
      return { ok: false, issue: "malformed-header" };
    }
    if (isJpegSofMarker(marker)) {
      if (segmentLength < 8) return { ok: false, issue: "malformed-header" };
      return validMetadata(
        "jpeg",
        "image/jpeg",
        readU16Be(bytes, offset + 5),
        readU16Be(bytes, offset + 3),
      );
    }
    offset += segmentLength;
  }
  return { ok: false, issue: "malformed-header" };
}

function parseWebpMetadata(bytes: Uint8Array): ManagedImageHeaderResult {
  if (bytes.length < 20) return { ok: false, issue: "malformed-header" };
  const riffSize = readU32Le(bytes, 4);
  const chunkSize = readU32Le(bytes, 16);
  if (riffSize === null || chunkSize === null || riffSize < 12 + chunkSize) {
    return { ok: false, issue: "malformed-header" };
  }
  const chunk = String.fromCharCode(bytes[12], bytes[13], bytes[14], bytes[15]);
  if (chunk === "VP8X") {
    if (chunkSize < 10 || bytes.length < 30) return { ok: false, issue: "malformed-header" };
    const widthMinusOne = readU24Le(bytes, 24);
    const heightMinusOne = readU24Le(bytes, 27);
    return validMetadata(
      "webp",
      "image/webp",
      widthMinusOne === null ? null : widthMinusOne + 1,
      heightMinusOne === null ? null : heightMinusOne + 1,
    );
  }
  if (chunk === "VP8L") {
    if (chunkSize < 5 || bytes.length < 25 || bytes[20] !== 0x2f) {
      return { ok: false, issue: "malformed-header" };
    }
    const width = 1 + bytes[21] + ((bytes[22] & 0x3f) << 8);
    const height = 1 + (bytes[22] >> 6) + (bytes[23] << 2) + ((bytes[24] & 0x0f) << 10);
    return validMetadata("webp", "image/webp", width, height);
  }
  if (chunk === "VP8 ") {
    if (chunkSize < 10 || bytes.length < 30 || !matchesBytes(bytes, 23, [0x9d, 0x01, 0x2a])) {
      return { ok: false, issue: "malformed-header" };
    }
    const rawWidth = readU16Le(bytes, 26);
    const rawHeight = readU16Le(bytes, 28);
    return validMetadata(
      "webp",
      "image/webp",
      rawWidth === null ? null : rawWidth & 0x3fff,
      rawHeight === null ? null : rawHeight & 0x3fff,
    );
  }
  return { ok: false, issue: "malformed-header" };
}

/** Parse bounded PNG, JPEG SOF, or WebP header metadata without decoding image pixels. */
export function parseManagedImageMetadata(bytes: Uint8Array): ManagedImageHeaderResult {
  if (matchesBytes(bytes, 0, PNG_SIGNATURE)) return parsePngMetadata(bytes);
  if (matchesBytes(bytes, 0, [0xff, 0xd8])) return parseJpegMetadata(bytes);
  if (matchesBytes(bytes, 0, [0x52, 0x49, 0x46, 0x46])
    && matchesBytes(bytes, 8, [0x57, 0x45, 0x42, 0x50])) {
    return parseWebpMetadata(bytes);
  }
  return { ok: false, issue: "unsupported-signature" };
}

/** Return a decoded-geometry issue for an otherwise valid managed image. */
export function managedImageDimensionIssue(
  metadata: Pick<ManagedImageMetadata, "width" | "height">,
): Extract<ManagedImageFileIssue, "dimensions-too-large" | "too-many-pixels"> | null {
  if (
    metadata.width > MANAGED_IMAGE_MAX_DIMENSION_PIXELS
    || metadata.height > MANAGED_IMAGE_MAX_DIMENSION_PIXELS
  ) {
    return "dimensions-too-large";
  }
  return metadata.width * metadata.height > MANAGED_IMAGE_MAX_TOTAL_PIXELS
    ? "too-many-pixels"
    : null;
}

/** Inspect a bounded image header before the WebView allocates its full data URL. */
export async function inspectManagedImageFile(
  file: BrowserImageFile,
  maxBytes: number,
): Promise<ManagedImageFileInspection> {
  if (file.size <= 0) return { ok: false, issue: "empty" };
  const declaredMimeType = file.type.trim().toLowerCase();
  if (declaredMimeType && !MANAGED_IMAGE_MIME_TYPES.has(declaredMimeType)) {
    return { ok: false, issue: "unsupported-type" };
  }
  if (file.size > maxBytes) return { ok: false, issue: "too-large" };

  let bytes: Uint8Array;
  try {
    const header = await file.slice(0, MANAGED_IMAGE_HEADER_MAX_BYTES).arrayBuffer();
    bytes = new Uint8Array(header);
  } catch {
    return { ok: false, issue: "invalid-image" };
  }
  const result = parseManagedImageMetadata(bytes);
  if (!result.ok) return { ok: false, issue: "invalid-image" };
  if (declaredMimeType && result.metadata.mimeType !== declaredMimeType) {
    return { ok: false, issue: "invalid-image" };
  }
  const dimensionIssue = managedImageDimensionIssue(result.metadata);
  return dimensionIssue
    ? { ok: false, issue: dimensionIssue }
    : { ok: true, metadata: result.metadata };
}

/** Return why a browser-selected image fails bounded header and geometry inspection. */
export async function managedImageFileIssue(
  file: BrowserImageFile,
  maxBytes: number,
): Promise<ManagedImageFileIssue | null> {
  const inspection = await inspectManagedImageFile(file, maxBytes);
  return inspection.ok ? null : inspection.issue;
}

/** Replace an uncertain FileReader MIME prefix with the canonical sniffed MIME type. */
export function normalizeManagedImageDataUrl(
  dataUrl: string,
  mimeType: ManagedImageMimeType,
): string | null {
  if (!dataUrl.startsWith("data:")) return null;
  const separatorIndex = dataUrl.indexOf(",");
  if (separatorIndex < 0) return null;
  const parameters = dataUrl.slice(5, separatorIndex).toLowerCase().split(";");
  if (!parameters.includes("base64")) return null;
  return `data:${mimeType};base64,${dataUrl.slice(separatorIndex + 1)}`;
}

/** Return whether reading a browser-selected file would exceed a byte limit. */
export function browserFileExceedsLimit(
  file: Pick<BrowserFileMetadata, "size">,
  maxBytes: number,
): boolean {
  return file.size > maxBytes;
}
