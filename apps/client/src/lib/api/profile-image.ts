import { invoke } from "@tauri-apps/api/core";
import { invalidateAssetUrl, loadAssetUrl } from "$lib/api/asset-url-cache";

/** A managed profile picture file that native code saved. */
export interface ProfileImageAsset {
  relativePath: string;
  /** Exact file size, which the `profile.image` preference records. */
  byteSize: number;
}

/** Loads a managed profile image through the bounded vault-scoped asset cache. */
export async function profileImageAssetUrl(relativePath: string): Promise<string> {
  return loadAssetUrl("profile-image", relativePath, () =>
    invoke<string>("profile_image_asset_data_url", { relativePath })
  );
}

/** Deletes a replaced managed profile image. */
export async function deleteProfileImageFile(relativePath: string): Promise<void> {
  await invoke("profile_image_delete_file", { relativePath });
  invalidateAssetUrl("profile-image", relativePath);
}
