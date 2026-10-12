import { invoke } from "@tauri-apps/api/core";
import { ensureDbUrl } from "$lib/api/db";
import { invalidateAssetUrl, loadAssetUrl } from "$lib/api/asset-url-cache";

export interface ProjectIconAsset {
  relativePath: string;
}

export async function pickProjectIconImageFile(): Promise<ProjectIconAsset | null> {
  return invoke<ProjectIconAsset | null>("projects_icon_pick_image_file");
}

export async function saveProjectIconImageDataUrl(dataUrl: string): Promise<ProjectIconAsset> {
  return invoke<ProjectIconAsset>("projects_icon_save_image_data_url", { dataUrl });
}

export async function downloadProjectIconImageUrl(url: string): Promise<ProjectIconAsset> {
  return invoke<ProjectIconAsset>("projects_icon_download_image_url", { url });
}

export async function projectIconAssetUrl(relativePath: string): Promise<string> {
  return loadAssetUrl("project-icon", relativePath, () =>
    invoke<string>("projects_icon_asset_data_url", { relativePath })
  );
}

export async function deleteProjectIconAssetsIfUnreferenced(relativePaths: string[]): Promise<void> {
  const dbUrl = await ensureDbUrl();
  await invoke("projects_icon_delete_assets_if_unreferenced", { dbUrl, relativePaths });
  for (const relativePath of relativePaths) {
    invalidateAssetUrl("project-icon", relativePath);
  }
}
