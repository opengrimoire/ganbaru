import { PROFILE_DISPLAY_NAME_FALLBACK } from "$lib/stores/preference-options";

/**
 * Resolve the visible local user label for Notes surfaces that need an
 * author name even when the app profile display name is empty.
 */
export function notesLocalUserDisplayName(displayName: string): string {
  return displayName.trim() || PROFILE_DISPLAY_NAME_FALLBACK;
}
