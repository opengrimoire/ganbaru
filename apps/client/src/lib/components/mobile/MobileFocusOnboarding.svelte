<script lang="ts">
  import Check from "@lucide/svelte/icons/check";
  import LoaderCircle from "@lucide/svelte/icons/loader-circle";
  import { onMount } from "svelte";
  import { fly } from "svelte/transition";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import Loading from "$lib/components/ui/Loading.svelte";
  import Notice from "$lib/components/ui/Notice.svelte";
  import FocusAccessGuide from "./focus-access/FocusAccessGuide.svelte";
  import MobileOnboardingProgress from "./MobileOnboardingProgress.svelte";
  import {
    FOCUS_ACCESS_STEP_ORDER,
    focusAccessGranted,
    focusAccessSensitive,
    focusAccessSteps,
    type FocusAccessStatuses,
    type FocusAccessStep,
  } from "$lib/mobile/focus-access-steps";
  import { onboardingPrimaryAction } from "$lib/vault/onboarding-primary-action";
  import { prefersReducedMotion } from "$lib/utils/smooth-swap";
  import {
    mobileDistractionsAccessStatus,
    openMobileDistractionsAccessibilitySettings,
    openMobileDistractionsUsageAccessSettings,
  } from "$lib/scheduling/mobile-distractions";
  import {
    completeMobileFocusOnboarding,
    markMobileFocusAccessReviewed,
    mobileBackgroundExecutionStatus,
    openMobileBackgroundExecutionSettings,
  } from "$lib/scheduling/mobile-background-execution";
  import {
    mobileNotificationAccessStatus,
    openMobileExactAlarmSettings,
    requestMobileNotificationPermission,
    resolveMobileNotificationAccess,
  } from "$lib/scheduling/mobile-notification-access";

  /**
   * Android Focus access onboarding: one screen per access Android does not already report as on, each with why it
   * matters, a drawing of the system screens it opens, and, for sensitive access, what Ganbaru sees and never sees.
   * The screen for sensitive access is its disclosure, shown immediately before the system settings open.
   */
  let { onComplete }: { onComplete: () => void } = $props();

  /** How long a step shows that its access is on before the next step arrives. */
  const GRANTED_HOLD_MS = 700;
  const { t } = getLocalization();
  const storage = safeStorage();
  const enterMs = prefersReducedMotion() ? 0 : 260;

  /** Where the current step stands after the person acted on it. */
  type StepOutcome = "idle" | "returned" | "granted";

  let statuses = $state<FocusAccessStatuses>({ background: null, notifications: null, distractions: null });
  let steps = $state<FocusAccessStep[] | null>(null);
  let index = $state(0);
  let outcome = $state<StepOutcome>("idle");
  let opening = $state(false);
  let openFailed = $state(false);
  let unavailable = $state(false);
  let pendingReview = $state<FocusAccessStep | null>(null);
  let leftForReview = $state(false);
  let grantedTimer = 0;

  const current = $derived(steps?.[index] ?? null);
  const appLabel = $derived(statuses.background?.appLabel ?? t("common.appName"));
  /** Whether Android can tell if the current step's access is on; autostart and some battery savers cannot. */
  const detectable = $derived(current !== null && focusAccessGranted(current, statuses) !== null);
  const continueAfterReturn = $derived(outcome === "returned" && !detectable);

  function safeStorage(): Storage | undefined {
    try {
      return window.localStorage;
    } catch {
      return undefined;
    }
  }

  function recordReviewed(step: FocusAccessStep): void {
    if (!storage) return;
    try {
      markMobileFocusAccessReviewed(step, storage);
    } catch (error) {
      console.warn("Android Focus access review could not be saved", error);
    }
  }

  async function refresh(): Promise<void> {
    const [background, notifications, distractions] = await Promise.allSettled([
      mobileBackgroundExecutionStatus(),
      mobileNotificationAccessStatus(),
      mobileDistractionsAccessStatus(),
    ]);
    const failures = [background, notifications, distractions].filter((result) => result.status === "rejected");
    for (const failure of failures) console.warn("Android Focus access status failed", failure.reason);
    unavailable = failures.length > 0;
    statuses = {
      background: background.status === "fulfilled" ? background.value : statuses.background,
      notifications: notifications.status === "fulfilled" ? notifications.value : statuses.notifications,
      distractions: distractions.status === "fulfilled" ? distractions.value : statuses.distractions,
    };
  }

  /** Rereads every access status for the error notice and reports whether all of them are readable now. */
  async function retryStatus(): Promise<boolean> {
    await refresh();
    return !unavailable;
  }

  function complete(): void {
    if (storage) {
      try {
        completeMobileFocusOnboarding(storage);
      } catch (error) {
        console.warn("Android Focus onboarding completion could not be saved", error);
      }
    }
    onComplete();
  }

  function advance(): void {
    window.clearTimeout(grantedTimer);
    if (!steps || index >= steps.length - 1) {
      complete();
      return;
    }
    index += 1;
    outcome = "idle";
    openFailed = false;
  }

  /** Records what the person did on the current step, and moves on by itself once Android reports the access on. */
  function settle(step: FocusAccessStep): void {
    if (current !== step) return;
    if (focusAccessGranted(step, statuses) === true) {
      outcome = "granted";
      grantedTimer = window.setTimeout(advance, GRANTED_HOLD_MS);
    } else {
      outcome = "returned";
    }
  }

  async function finishReturnedReview(): Promise<void> {
    const step = pendingReview;
    if (!step || !leftForReview) return;
    pendingReview = null;
    leftForReview = false;
    recordReviewed(step);
    await refresh();
    settle(step);
  }

  /** Opens the system screen for the step, or asks for notifications in place while Android still can. */
  async function openStep(step: FocusAccessStep): Promise<void> {
    if (opening) return;
    opening = true;
    openFailed = false;
    try {
      if (step === "notifications" && statuses.notifications?.permission === "prompt") {
        statuses = { ...statuses, notifications: await requestMobileNotificationPermission() };
        recordReviewed(step);
        settle(step);
        return;
      }
      pendingReview = step;
      leftForReview = false;
      await openSettings(step);
    } catch (error) {
      pendingReview = null;
      openFailed = true;
      console.warn(`Android settings for ${step} failed to open`, error);
    } finally {
      opening = false;
    }
  }

  async function openSettings(step: FocusAccessStep): Promise<void> {
    switch (step) {
      case "notifications":
        if (statuses.notifications) await resolveMobileNotificationAccess(statuses.notifications);
        return;
      case "exact-alarm":
        return openMobileExactAlarmSettings();
      case "autostart":
      case "battery":
        return openMobileBackgroundExecutionSettings(step);
      case "usage-access":
        return openMobileDistractionsUsageAccessSettings();
      case "app-blocking":
        return openMobileDistractionsAccessibilitySettings();
    }
  }

  function stepTitle(step: FocusAccessStep): string {
    switch (step) {
      case "notifications": return t("mobile.focusOnboarding.notificationsTitle");
      case "exact-alarm": return t("mobile.focusOnboarding.exactAlarmTitle");
      case "autostart": return t("mobile.focusOnboarding.autostartTitle");
      case "battery": return t("mobile.focusOnboarding.batteryTitle");
      case "usage-access": return t("mobile.focusOnboarding.usageAccessTitle");
      case "app-blocking": return t("mobile.focusOnboarding.appBlockingTitle");
    }
  }

  function stepBody(step: FocusAccessStep): string {
    switch (step) {
      case "notifications": return t("mobile.focusOnboarding.notificationsBody", appLabel);
      case "exact-alarm": return t("mobile.focusOnboarding.exactAlarmBody");
      case "autostart": return t("mobile.focusOnboarding.autostartBody", appLabel);
      case "battery": return t("mobile.focusOnboarding.batteryBody", appLabel);
      case "usage-access": return t("mobile.focusOnboarding.usageAccessBody", appLabel);
      case "app-blocking": return t("mobile.focusOnboarding.appBlockingBody", appLabel);
    }
  }

  /** What sensitive access lets Ganbaru see, and what it never sees. */
  function stepBoundary(step: FocusAccessStep): { sees: string; neverSees: string } | null {
    if (!focusAccessSensitive(step)) return null;
    return step === "usage-access"
      ? { sees: t("mobile.focusOnboarding.usageAccessSees"), neverSees: t("mobile.focusOnboarding.usageAccessNeverSees") }
      : { sees: t("mobile.focusOnboarding.appBlockingSees"), neverSees: t("mobile.focusOnboarding.appBlockingNeverSees") };
  }

  function primaryLabel(step: FocusAccessStep): string {
    if (outcome === "granted") return t("mobile.focusOnboarding.done");
    if (continueAfterReturn) return t("mobile.focusOnboarding.continue");
    const action = step === "notifications" && statuses.notifications?.permission === "prompt"
      ? t("mobile.focusOnboarding.notificationsAction")
      : t("mobile.focusOnboarding.openSettings");
    return outcome === "returned" && detectable ? t("mobile.focusOnboarding.stillOffAction", action) : action;
  }

  onMount(() => {
    const handleVisibility = (): void => {
      if (document.visibilityState === "hidden" && pendingReview) leftForReview = true;
      else if (document.visibilityState === "visible") void finishReturnedReview();
    };
    const handleBlur = (): void => {
      if (pendingReview) leftForReview = true;
    };
    const handleFocus = (): void => {
      void finishReturnedReview();
    };

    void refresh().then(() => {
      steps = focusAccessSteps(statuses);
      if (steps.length === 0) complete();
    });
    document.addEventListener("visibilitychange", handleVisibility);
    window.addEventListener("blur", handleBlur);
    window.addEventListener("focus", handleFocus);

    return () => {
      window.clearTimeout(grantedTimer);
      document.removeEventListener("visibilitychange", handleVisibility);
      window.removeEventListener("blur", handleBlur);
      window.removeEventListener("focus", handleFocus);
    };
  });
</script>

<main
  class="mobile-viewport-height flex w-screen flex-col overflow-hidden bg-background pb-(--safe-area-bottom) pt-(--safe-area-top) text-foreground"
  style="padding-left: var(--safe-area-left); padding-right: var(--safe-area-right);"
>
  <MobileOnboardingProgress stage="permissions" screen={current ? FOCUS_ACCESS_STEP_ORDER.indexOf(current) : 0} />
  <section class="min-h-0 flex-1 overflow-y-auto overflow-x-hidden px-4 min-[560px]:px-8">
    {#if steps === null || current === null}
      <div class="flex h-full items-center justify-center text-sm">
        <Loading layout="inline" showLabel label={t("mobile.focusOnboarding.checkingAccess")} />
      </div>
    {:else}
      {@const boundary = stepBoundary(current)}
      <div class="mx-auto flex min-h-full w-full max-w-md flex-col pb-5">
        {#if steps.length > 1}
          <p class="sr-only">{t("mobile.focusOnboarding.permissionProgress", index + 1, steps.length)}</p>
        {/if}

        {#key current}
          <div class="flex flex-1 flex-col pt-8" in:fly={{ x: 24, duration: enterMs }}>
            <div class="space-y-2">
              <h1 class="text-2xl font-semibold leading-tight text-foreground">{stepTitle(current)}</h1>
              <p class="text-sm leading-6 text-muted-foreground">{stepBody(current)}</p>
              {#if boundary}
                <!-- The data line comes last so the two lines about what the app does follow its description. -->
                <p class="text-sm leading-6 text-muted-foreground">{boundary.sees}</p>
                <p class="text-sm leading-6 text-muted-foreground">{boundary.neverSees}</p>
                <p class="text-sm leading-6 text-muted-foreground">{t("mobile.focusOnboarding.privacyPoint")}</p>
              {/if}
              {#if current === "battery" && statuses.background?.backgroundRestricted}
                <p class="text-sm leading-6 text-foreground">{t("mobile.focusOnboarding.backgroundRestricted")}</p>
              {/if}
            </div>

            <!-- The body text's line spacing adds about 4px above, so the top padding is one step smaller to center the guide. -->
            <div class="flex flex-1 flex-col pb-8 pt-7">
              <FocusAccessGuide step={current} {appLabel} />
            </div>
          </div>
        {/key}

        <div class="flex flex-col gap-1">
          {#if outcome === "returned" && detectable}
            <p class="sr-only" role="status">{t("mobile.focusOnboarding.stillOff")}</p>
          {/if}
          <button
            use:onboardingPrimaryAction
            type="button"
            class="flex min-h-11 w-full items-center justify-center gap-2 rounded-md bg-primary px-3 py-2 text-sm font-medium text-primary-foreground transition-colors active:bg-primary/90 disabled:cursor-wait"
            disabled={opening || outcome === "granted"}
            aria-busy={opening || undefined}
            onclick={() => (continueAfterReturn ? advance() : void openStep(current))}
          >
            {#if opening}
              <LoaderCircle class="size-4 animate-spin" aria-hidden="true" />
            {:else if outcome === "granted"}
              <Check class="size-4" aria-hidden="true" />
            {/if}
            {primaryLabel(current)}
          </button>
          <button
            type="button"
            class="flex min-h-11 w-full items-center justify-center rounded-md px-3 py-2 text-sm font-medium text-muted-foreground transition-colors active:text-foreground disabled:opacity-50"
            disabled={opening || outcome === "granted"}
            onclick={() => (continueAfterReturn ? void openStep(current) : advance())}
          >
            {continueAfterReturn ? t("mobile.focusOnboarding.openAgain") : t("mobile.focusOnboarding.notNow")}
          </button>
          {#if openFailed}
            <p class="pt-1 text-center text-sm text-destructive" role="alert">{t("mobile.focusOnboarding.openError")}</p>
          {:else if unavailable}
            <Notice class="pt-1 text-sm" message={t("mobile.focusOnboarding.statusError")} copy={false} onRetry={retryStatus} />
          {/if}
        </div>
      </div>
    {/if}
  </section>
</main>
