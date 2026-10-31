import { invalidateAssetUrlKind } from "$lib/api/asset-url-cache";
import { listenSyncApplied } from "$lib/api/sync";
import { PREFERENCES_SYNC_TABLE } from "$lib/vault/config";

let revision = $state(0);
let syncStarted = false;

/**
 * Profile picture files that sync may replace or deliver after the preference naming them arrived. Avatars read
 * `revision` so they load the picture again once its file is present.
 */
export const profileImageFiles = {
  /** Changes whenever profile picture files may have changed outside this window. */
  get revision(): number {
    return revision;
  },
};

/** Drop cached profile pictures and tell avatars to load them again. */
export function profileImageFilesChanged(): void {
  invalidateAssetUrlKind("profile-image");
  revision += 1;
}

/** Start reloading avatars when sync applies preference rows or their picture files. Idempotent. */
export function startProfileImageFileSync(): void {
  if (syncStarted) return;
  syncStarted = true;
  void listenSyncApplied((tables) => {
    if (tables.includes(PREFERENCES_SYNC_TABLE)) profileImageFilesChanged();
  }).catch((error: unknown) => {
    syncStarted = false;
    console.error("Profile pictures could not listen for sync", error);
  });
}
