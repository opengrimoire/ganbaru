import { MANAGED_ICON_IMAGE_MAX_MEGABYTES, type ManagedImageFileIssue } from "$lib/browser-file-policy";
import type { Translate } from "$lib/i18n/translator.svelte";

/**
 * Localized explanation for a profile picture that failed browser-file inspection.
 *
 * @param t Translator for the active locale.
 * @param issue Inspection issue code.
 * @returns One user-facing sentence.
 */
export function profileImageIssueMessage(t: Translate, issue: ManagedImageFileIssue): string {
  switch (issue) {
    case "empty":
      return t("settings.profileIdentity.pictureIssue.empty");
    case "unsupported-type":
      return t("settings.profileIdentity.pictureIssue.unsupportedType");
    case "too-large":
      return t("settings.profileIdentity.pictureIssue.tooLarge", MANAGED_ICON_IMAGE_MAX_MEGABYTES);
    case "dimensions-too-large":
      return t("settings.profileIdentity.pictureIssue.dimensionsTooLarge");
    case "too-many-pixels":
      return t("settings.profileIdentity.pictureIssue.tooManyPixels");
    case "invalid-image":
      return t("settings.profileIdentity.pictureIssue.invalidImage");
  }
}
