import type { MobileBackgroundExecutionStatus, MobileFocusAccessReview } from "$lib/scheduling/mobile-background-execution";
import type { MobileDistractionsAccessStatus } from "$lib/scheduling/mobile-distractions";
import type { MobileNotificationAccessStatus } from "$lib/scheduling/mobile-notification-access";

/** One Android access the Focus onboarding asks for, each on its own screen. */
export type FocusAccessStep = MobileFocusAccessReview;

/** The access statuses Android reported; `null` when a status could not be read. */
export interface FocusAccessStatuses {
  background: MobileBackgroundExecutionStatus | null;
  notifications: MobileNotificationAccessStatus | null;
  distractions: MobileDistractionsAccessStatus | null;
}

/** Steps in the order they are asked: delivery first, then background survival, then the sensitive app controls. */
const STEP_ORDER: readonly FocusAccessStep[] = [
  "notifications",
  "exact-alarm",
  "autostart",
  "battery",
  "usage-access",
  "app-blocking",
];

/**
 * Whether Android reports the step's access as on. `null` means Android cannot tell: MIUI and similar autostart
 * switches have no public query, and a battery saver can restrict an app that is not on the standard exemption list.
 */
export function focusAccessGranted(step: FocusAccessStep, statuses: FocusAccessStatuses): boolean | null {
  const { background, notifications, distractions } = statuses;
  switch (step) {
    case "notifications":
      return notifications ? notifications.permission === "granted" : null;
    case "exact-alarm":
      return notifications ? !notifications.exactAlarm.required || notifications.exactAlarm.granted : null;
    case "autostart":
      return null;
    case "battery":
      return background?.batteryOptimizationExempt && !background.backgroundRestricted ? true : null;
    case "usage-access":
      return distractions ? distractions.usageAccess : null;
    case "app-blocking":
      return distractions ? distractions.accessibility : null;
  }
}

/** Whether the step applies to this device at all, before asking whether its access is already on. */
function focusAccessApplies(step: FocusAccessStep, statuses: FocusAccessStatuses): boolean {
  switch (step) {
    case "exact-alarm":
      return statuses.notifications?.exactAlarm.required ?? false;
    case "autostart":
      return statuses.background?.autostartSettingsAvailable ?? false;
    default:
      return true;
  }
}

/**
 * The steps to show, in order: every step that applies to this device and whose access Android does not already
 * report as on. A status that could not be read keeps its step, so the person can still review it.
 */
export function focusAccessSteps(statuses: FocusAccessStatuses): FocusAccessStep[] {
  return STEP_ORDER.filter((step) => focusAccessApplies(step, statuses) && focusAccessGranted(step, statuses) !== true);
}

/** The steps that ask for sensitive access and therefore show what Ganbaru sees and never sees. */
export function focusAccessSensitive(step: FocusAccessStep): boolean {
  return step === "usage-access" || step === "app-blocking";
}

/** One moment of a step's guide animation: how long it holds, and which instruction it illustrates. */
export interface FocusAccessGuidePhase {
  durationMs: number;
  caption: FocusAccessGuideCaption;
}

/** The instruction a guide phase shows under the animation. */
export type FocusAccessGuideCaption =
  | "allow"
  | "search"
  | "turn-on"
  | "no-restrictions"
  | "accept-warning"
  | "open-downloaded"
  | "choose-service"
  | "tap-allow";

const TAP_MS = 750;

/**
 * The guide animation for each step, as phases a scene draws by index. Taps hold briefly so the person sees where to
 * press before the result appears, and the last phase holds longest before the guide starts over.
 */
export const FOCUS_ACCESS_GUIDES: Readonly<Record<FocusAccessStep, readonly FocusAccessGuidePhase[]>> = {
  notifications: [
    { durationMs: 900, caption: "allow" },
    { durationMs: 2_200, caption: "allow" },
    { durationMs: 2_400, caption: "allow" },
  ],
  "exact-alarm": [
    { durationMs: 1_200, caption: "turn-on" },
    { durationMs: TAP_MS, caption: "turn-on" },
    { durationMs: 2_000, caption: "turn-on" },
  ],
  autostart: [
    { durationMs: 1_000, caption: "search" },
    { durationMs: 1_500, caption: "search" },
    { durationMs: TAP_MS, caption: "turn-on" },
    { durationMs: 2_000, caption: "turn-on" },
  ],
  battery: [
    { durationMs: 1_400, caption: "no-restrictions" },
    { durationMs: TAP_MS, caption: "no-restrictions" },
    { durationMs: 2_200, caption: "no-restrictions" },
  ],
  "usage-access": [
    { durationMs: 1_100, caption: "turn-on" },
    { durationMs: TAP_MS, caption: "turn-on" },
    { durationMs: 1_300, caption: "accept-warning" },
    { durationMs: TAP_MS, caption: "accept-warning" },
    { durationMs: 900, caption: "accept-warning" },
    { durationMs: TAP_MS, caption: "accept-warning" },
    { durationMs: 2_000, caption: "accept-warning" },
  ],
  "app-blocking": [
    { durationMs: 1_100, caption: "open-downloaded" },
    { durationMs: TAP_MS, caption: "open-downloaded" },
    { durationMs: 1_100, caption: "choose-service" },
    { durationMs: TAP_MS, caption: "choose-service" },
    { durationMs: 1_000, caption: "turn-on" },
    { durationMs: TAP_MS, caption: "turn-on" },
    { durationMs: 1_200, caption: "tap-allow" },
    { durationMs: TAP_MS, caption: "tap-allow" },
    { durationMs: 2_000, caption: "tap-allow" },
  ],
};
