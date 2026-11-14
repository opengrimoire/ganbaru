import { invoke } from "@tauri-apps/api/core";
import {
  inspectManagedImageFile,
  MANAGED_ICON_IMAGE_MAX_BYTES,
  MANAGED_IMAGE_FILE_ACCEPT,
  normalizeManagedImageDataUrl,
} from "$lib/browser-file-policy";
import type { ProfileImageAsset } from "$lib/api/profile-image";
import { translate } from "$lib/i18n/translator.svelte";
import { LocalizedError } from "$lib/i18n/user-errors";
import { profileImageIssueMessage } from "$lib/profile/image-issue";

function readFileAsDataUrl(file: File): Promise<string> {
  return new Promise((resolve, reject) => {
    const reader = new FileReader();
    reader.addEventListener("load", () => {
      if (typeof reader.result === "string") {
        resolve(reader.result);
        return;
      }
      reject(new LocalizedError(translate("settings.profileIdentity.pictureIssue.readFailed")));
    }, { once: true });
    reader.addEventListener("error", () => {
      reject(new LocalizedError(translate("settings.profileIdentity.pictureIssue.readFailed")));
    }, { once: true });
    reader.readAsDataURL(file);
  });
}

function chooseBrowserImage(title: string): Promise<File | null> {
  return new Promise((resolve) => {
    const input = document.createElement("input");
    input.type = "file";
    input.accept = MANAGED_IMAGE_FILE_ACCEPT;
    input.setAttribute("aria-label", title);
    input.style.position = "fixed";
    input.style.left = "-10000px";
    document.body.append(input);

    let settled = false;
    const finish = (file: File | null): void => {
      if (settled) return;
      settled = true;
      window.removeEventListener("focus", handleWindowFocus);
      input.remove();
      resolve(file);
    };
    const handleWindowFocus = (): void => {
      window.setTimeout(() => finish(input.files?.item(0) ?? null), 250);
    };

    input.addEventListener("change", () => finish(input.files?.item(0) ?? null), { once: true });
    input.addEventListener("cancel", () => finish(null), { once: true });
    window.addEventListener("focus", handleWindowFocus, { once: true });
    input.click();
  });
}

/** Open Android's document UI and copy a validated image into managed profile assets. */
export async function pickProfileImageFile(title: string): Promise<ProfileImageAsset | null> {
  const file = await chooseBrowserImage(title);
  if (!file) return null;

  const inspection = await inspectManagedImageFile(file, MANAGED_ICON_IMAGE_MAX_BYTES);
  if (!inspection.ok) throw new LocalizedError(profileImageIssueMessage(translate, inspection.issue));

  const rawDataUrl = await readFileAsDataUrl(file);
  const dataUrl = normalizeManagedImageDataUrl(rawDataUrl, inspection.metadata.mimeType);
  if (!dataUrl) throw new LocalizedError(translate("settings.profileIdentity.pictureIssue.encodeFailed"));
  return invoke<ProfileImageAsset>("profile_image_save_data_url", { dataUrl });
}
