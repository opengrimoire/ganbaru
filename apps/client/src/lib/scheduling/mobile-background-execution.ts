import { invoke } from "@tauri-apps/api/core";

export type MobileBackgroundSettingsDestination = "autostart" | "battery";
export type MobileFocusAccessReview =
  | MobileBackgroundSettingsDestination
  | "notifications"
  | "exact-alarm"
  | "usage-access"
  | "app-blocking";

const MOBILE_FOCUS_ONBOARDING_STORAGE_KEY = "ganbaru.mobile-focus-onboarding.v1";
const MOBILE_FOCUS_REVIEW_STORAGE_PREFIX = "ganbaru.mobile-focus-review.v1";
type ReadableStorage = Pick<Storage, "getItem">;
type WritableStorage = Pick<Storage, "setItem">;

export interface MobileBackgroundExecutionStatus {
  manufacturer: string;
  /** The launcher label of this build, such as Ganbaru or Ganbaru Dev, as Android settings list it. */
  appLabel: string;
  autostartSettingsAvailable: boolean;
  backgroundRestricted: boolean;
  batteryOptimizationExempt: boolean;
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}

/** Validate the bounded background-execution status returned by the Android bridge. */
export function parseMobileBackgroundExecutionStatus(
  value: unknown,
): MobileBackgroundExecutionStatus {
  if (!isRecord(value)) throw new Error("Android background status must be an object");
  const {
    manufacturer,
    appLabel,
    autostartSettingsAvailable,
    backgroundRestricted,
    batteryOptimizationExempt,
  } = value;
  if (typeof manufacturer !== "string" || manufacturer.length > 80) {
    throw new Error("Android background status has an invalid manufacturer");
  }
  if (typeof appLabel !== "string" || appLabel.trim().length === 0 || appLabel.length > 80) {
    throw new Error("Android background status has an invalid app label");
  }
  if (typeof autostartSettingsAvailable !== "boolean") {
    throw new Error("Android background status has an invalid autostart availability");
  }
  if (typeof backgroundRestricted !== "boolean") {
    throw new Error("Android background status has an invalid restriction state");
  }
  if (typeof batteryOptimizationExempt !== "boolean") {
    throw new Error("Android background status has an invalid battery optimization state");
  }
  return {
    manufacturer,
    appLabel,
    autostartSettingsAvailable,
    backgroundRestricted,
    batteryOptimizationExempt,
  };
}

/** Read Android background-execution state visible through public platform APIs. */
export async function mobileBackgroundExecutionStatus(): Promise<MobileBackgroundExecutionStatus> {
  const value = await invoke<unknown>("mobile_notification_background_execution_status");
  return parseMobileBackgroundExecutionStatus(value);
}

/** Open the requested Android or manufacturer-specific background settings screen. */
export async function openMobileBackgroundExecutionSettings(
  destination: MobileBackgroundSettingsDestination,
): Promise<void> {
  await invoke("mobile_notification_open_background_execution_settings", { destination });
}

/** Return whether the current Android Focus onboarding has been completed. */
export function mobileFocusOnboardingCompleted(storage: ReadableStorage | undefined): boolean {
  return storage?.getItem(MOBILE_FOCUS_ONBOARDING_STORAGE_KEY) === "complete";
}

/** Persist completion of the current Android Focus onboarding. */
export function completeMobileFocusOnboarding(storage: WritableStorage): void {
  storage.setItem(MOBILE_FOCUS_ONBOARDING_STORAGE_KEY, "complete");
}

function mobileFocusReviewStorageKey(review: MobileFocusAccessReview): string {
  return `${MOBILE_FOCUS_REVIEW_STORAGE_PREFIX}.${review}`;
}

/** Return whether the user has returned from reviewing a Focus access control. */
export function mobileFocusAccessReviewed(
  review: MobileFocusAccessReview,
  storage: ReadableStorage | undefined,
): boolean {
  return storage?.getItem(mobileFocusReviewStorageKey(review)) === "reviewed";
}

/** Persist that the user returned from reviewing a Focus access control. */
export function markMobileFocusAccessReviewed(
  review: MobileFocusAccessReview,
  storage: WritableStorage,
): void {
  storage.setItem(mobileFocusReviewStorageKey(review), "reviewed");
}
