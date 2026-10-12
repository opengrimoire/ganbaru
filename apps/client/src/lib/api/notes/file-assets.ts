import { invoke } from "@tauri-apps/api/core";
import { ensureDbUrl } from "$lib/api/db";
import { invalidateAssetUrl, loadAssetUrl } from "$lib/api/asset-url-cache";
import type { NotesFileAssetMetadata, NotesMediaBlockType } from "$lib/notes/block-types/media";
import type {
  NotesImportFileDiagnostic,
  NotesImportFileAction,
} from "$lib/notes/import-file-policy";

interface NotesFileAssetDto {
  relativePath: string;
  originalName: string | null;
  contentType: string;
  byteSize: number;
  sha256: string;
  kind: NotesMediaBlockType;
}

interface NotesImportFileReferenceDto {
  action: NotesImportFileAction;
  asset: NotesFileAssetDto | null;
  externalUrl: string | null;
  diagnostics: NotesImportFileDiagnostic[];
}

function mapNotesFileAssetDto(value: NotesFileAssetDto): NotesFileAssetMetadata {
  return {
    relativePath: value.relativePath,
    originalName: value.originalName,
    contentType: value.contentType,
    byteSize: value.byteSize,
    sha256: value.sha256,
    kind: value.kind,
  };
}

/** Open a native file picker and copy the selected file into managed Notes assets. */
export async function pickNotesFileAsset(
  blockType: NotesMediaBlockType,
): Promise<NotesFileAssetMetadata | null> {
  const dbUrl = await ensureDbUrl();
  const asset = await invoke<NotesFileAssetDto | null>("notes_pick_file_asset", {
    dbUrl,
    blockType,
  });
  return asset ? mapNotesFileAssetDto(asset) : null;
}

/** Load a managed Notes file asset as a data URL for local preview rendering. */
export async function notesFileAssetUrl(relativePath: string): Promise<string> {
  return loadAssetUrl("notes-file", relativePath, async () => {
    const dbUrl = await ensureDbUrl();
    return invoke<string>("notes_file_asset_data_url", { dbUrl, relativePath });
  });
}

/** Releases a local Notes preview when it leaves the retained render range. */
export function releaseNotesFileAssetUrl(relativePath: string): void {
  invalidateAssetUrl("notes-file", relativePath);
}
