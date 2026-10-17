import type { NotesFileAssetMetadata } from "$lib/notes/block-types/media";

const MANAGED_FILE_PREFIX = "notes/files/";
const MANAGED_URL_SCHEME = "ganbaru-asset:";

/** A comment attachment reduced to the fields the discussion UI renders. */
export interface NotesCommentAttachmentFile {
  name: string;
  contentType: string | null;
  byteSize: number | null;
  /** Vault-relative managed path, or null for an external reference. */
  assetPath: string | null;
  /** External URL for imported references, or null for a managed file. */
  externalUrl: string | null;
  isImage: boolean;
}

/** Build the stored attachment payload for a file copied into managed Notes assets. */
export function notesCommentAttachmentFromAsset(
  asset: NotesFileAssetMetadata,
): Record<string, unknown> {
  const name = asset.originalName?.trim() || asset.relativePath.slice(asset.relativePath.lastIndexOf("/") + 1);
  return {
    category: "file",
    name,
    file: {
      url: `${MANAGED_URL_SCHEME}${asset.relativePath}`,
      name,
      content_type: asset.contentType,
      byte_size: asset.byteSize,
      sha256: asset.sha256,
      ganbaru_asset_path: asset.relativePath,
    },
  };
}

function readOptionalString(record: Record<string, unknown>, key: string): string | null {
  const value = record[key];
  return typeof value === "string" && value.trim() ? value.trim() : null;
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}

function externalHttpUrl(value: string | null): string | null {
  if (!value) return null;
  try {
    const url = new URL(value);
    return url.protocol === "https:" || url.protocol === "http:" ? url.href : null;
  } catch {
    return null;
  }
}

/**
 * Read a stored attachment as a renderable file, or null when it is not a file attachment.
 * Managed paths outside the Notes file directory are ignored rather than trusted.
 */
export function notesCommentAttachmentFile(
  attachment: Record<string, unknown>,
): NotesCommentAttachmentFile | null {
  const type = readOptionalString(attachment, "type");
  if (type && type !== "file") return null;
  const file = attachment.file;
  if (!isRecord(file)) return null;
  const assetPath = readOptionalString(file, "ganbaru_asset_path");
  const managedPath = assetPath?.startsWith(MANAGED_FILE_PREFIX) ? assetPath : null;
  const externalUrl = managedPath ? null : externalHttpUrl(readOptionalString(file, "url"));
  if (!managedPath && !externalUrl) return null;
  const contentType = readOptionalString(file, "content_type");
  const byteSize = typeof file.byte_size === "number" && Number.isInteger(file.byte_size) && file.byte_size >= 0
    ? file.byte_size
    : null;
  const fallbackName = managedPath ?? externalUrl ?? "";
  const name = readOptionalString(file, "name")
    ?? readOptionalString(attachment, "name")
    ?? fallbackName.slice(fallbackName.lastIndexOf("/") + 1);
  const category = readOptionalString(attachment, "category");
  return {
    name,
    contentType,
    byteSize,
    assetPath: managedPath,
    externalUrl,
    isImage: contentType?.startsWith("image/") === true || category === "image",
  };
}
