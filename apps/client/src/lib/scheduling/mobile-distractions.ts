import { invoke } from "@tauri-apps/api/core";
import { translate } from "$lib/i18n/translator.svelte";
import { flushConfig } from "$lib/vault/config";

const PLUGIN_COMMAND = "plugin:ganbaru-mobile-distractions";

/** Persist settings and send localized text. Rust owns rule derivation and accounting. */
export async function publishMobileDistractionsConfig(): Promise<void> {
  if (__GANBARU_AI_BUILD_PLATFORM__ !== "android") return;
  await flushConfig();
  await invoke("distractions_mobile_update_copy", {
    copy: {
      channelName: translate("settings.distractions.mobile.channelName"),
      channelDescription: translate("settings.distractions.mobile.channelDescription"),
      blockedMessage: translate("settings.distractions.mobile.blockedMessage"),
      limitMessage: translate("settings.distractions.mobile.limitMessage"),
    },
  });
}

export interface MobileDistractionsAccessStatus {
  usageAccess: boolean;
  accessibility: boolean;
}

export interface MobileDistractionsAppCandidate {
  name: string;
  packageName: string;
}


/** Read whether Android has granted the two accesses required for app enforcement. */
export async function mobileDistractionsAccessStatus(): Promise<MobileDistractionsAccessStatus> {
  return await invoke<MobileDistractionsAccessStatus>(`${PLUGIN_COMMAND}|accessStatus`);
}

/** Open Android's Usage Access settings with an app-details fallback. */
export async function openMobileDistractionsUsageAccessSettings(): Promise<void> {
  await invoke(`${PLUGIN_COMMAND}|openUsageAccessSettings`);
}

/** Open Android's Accessibility settings with an app-details fallback. */
export async function openMobileDistractionsAccessibilitySettings(): Promise<void> {
  await invoke(`${PLUGIN_COMMAND}|openAccessibilitySettings`);
}

/** List launchable Android applications that are safe to select for blocking. */
export async function listMobileDistractionsApps(): Promise<MobileDistractionsAppCandidate[]> {
  return await invoke<MobileDistractionsAppCandidate[]>(`${PLUGIN_COMMAND}|listLaunchableApps`);
}

/** Consume a notification deep-link target captured by the Android plugin. */
export async function takeMobileDistractionsNotificationAction(): Promise<"mobile" | "limits" | null> {
  const result = await invoke<{ target: "mobile" | "limits" | null }>(
    `${PLUGIN_COMMAND}|takeNotificationAction`,
  );
  return result.target;
}
