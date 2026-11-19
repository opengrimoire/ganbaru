<script lang="ts">
  import Check from "@lucide/svelte/icons/check";
  import { onMount } from "svelte";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import Loading from "$lib/components/ui/Loading.svelte";
  import Notice from "$lib/components/ui/Notice.svelte";
  import OnboardingNav, { type OnboardingNavPrimary } from "$lib/components/vault/OnboardingNav.svelte";
  import WelcomeSceneStep from "$lib/components/vault/welcome/WelcomeSceneStep.svelte";
  import { ONBOARDING_SECONDARY_BUTTON_CLASS } from "$lib/vault/welcome-layout";
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
   * The screen for sensitive access is its disclosure, shown immediately before the system settings open. The screens
   * share the welcome screens' navigation, with Back between them, and a screen whose access is already on, reached by
   * going back or forward, says so and only continues. Its first screen shows Back disabled, since the
   * screens before it cannot be chosen again.
   */
  let { onComplete }: { onComplete: () => void } = $props();

  /** How long a step shows that its access just turned on before the next step arrives. */
  const GRANTED_HOLD_MS = 700;
  const { t } = getLocalization();
  const storage = safeStorage();

  /** Where the current step stands: untouched, back from settings with the access still off, or with the access on. */
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

  /** Shows the step at `next`, already marked as on when Android reports its access on. */
  function arrive(next: number): void {
    window.clearTimeout(grantedTimer);
    const step = steps?.[next];
    if (!step) return;
    index = next;
    outcome = focusAccessGranted(step, statuses) === true ? "granted" : "idle";
    openFailed = false;
  }

  function advance(): void {
    window.clearTimeout(grantedTimer);
    if (!steps || index >= steps.length - 1) {
      complete();
      return;
    }
    arrive(index + 1);
  }

  function back(): void {
    if (index > 0) arrive(index - 1);
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

  /**
   * The step's primary action: continue once its access is on, or once the person came back from settings Android
   * cannot check; otherwise open its settings, or ask for notifications in place while Android still can.
   */
  function primaryAction(step: FocusAccessStep): OnboardingNavPrimary {
    if (outcome === "granted" || continueAfterReturn) {
      return { label: t("mobile.focusOnboarding.continue"), onClick: advance };
    }
    const action = step === "notifications" && statuses.notifications?.permission === "prompt"
      ? t("mobile.focusOnboarding.notificationsAction")
      : t("mobile.focusOnboarding.openSettings");
    return {
      label: outcome === "returned" && detectable ? t("mobile.focusOnboarding.stillOffAction", action) : action,
      onClick: () => void openStep(step),
      busy: opening,
      arrow: false,
    };
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
  <MobileOnboardingProgress stage="permissions" screen={current ? FOCUS_ACCESS_STEP_ORDER.indexOf(current) : 0}>
    {#if steps === null || current === null}
      <div class="flex h-full items-center justify-center text-sm">
        <Loading layout="inline" showLabel label={t("mobile.focusOnboarding.checkingAccess")} />
      </div>
    {:else}
      {@const step = current}
      {@const boundary = stepBoundary(step)}
      <div class="@container flex h-full flex-col">
        {#if steps.length > 1}
          <p class="sr-only">{t("mobile.focusOnboarding.permissionProgress", index + 1, steps.length)}</p>
        {/if}
        <div class="relative min-h-0 flex-1 overflow-hidden">
          <div class="absolute inset-0">
            {#key step}
              <WelcomeSceneStep headline={stepTitle(step)} body={stepBody(step)}>
                {#snippet details()}
                  {#if boundary}
                    <!-- The data line comes last so the two lines about what the app does follow its description. -->
                    <p class="text-sm leading-6 text-muted-foreground @min-[560px]:text-base @min-[560px]:leading-7">{boundary.sees}</p>
                    <p class="text-sm leading-6 text-muted-foreground @min-[560px]:text-base @min-[560px]:leading-7">{boundary.neverSees}</p>
                    <p class="text-sm leading-6 text-muted-foreground @min-[560px]:text-base @min-[560px]:leading-7">{t("mobile.focusOnboarding.privacyPoint")}</p>
                  {/if}
                  {#if step === "battery" && statuses.background?.backgroundRestricted}
                    <p class="text-sm leading-6 text-foreground @min-[560px]:text-base @min-[560px]:leading-7">{t("mobile.focusOnboarding.backgroundRestricted")}</p>
                  {/if}
                {/snippet}
                <FocusAccessGuide {step} {appLabel} />
              </WelcomeSceneStep>
            {/key}
          </div>
        </div>

        <OnboardingNav
          primary={primaryAction(step)}
          onBack={index > 0 ? back : undefined}
          backLocked
          backDisabled={opening}
        >
          {#snippet secondary()}
            {#if outcome === "granted"}
              <p class="flex min-h-11 items-center justify-center gap-2 text-sm font-medium text-foreground" role="status">
                <Check size={16} strokeWidth={2} class="shrink-0 text-primary" aria-hidden="true" />
                {t("mobile.focusOnboarding.turnedOn")}
              </p>
            {:else}
              {#if outcome === "returned" && detectable}
                <p class="sr-only" role="status">{t("mobile.focusOnboarding.stillOff")}</p>
              {/if}
              {#if openFailed}
                <p class="text-center text-sm text-destructive" role="alert">{t("mobile.focusOnboarding.openError")}</p>
              {:else if unavailable}
                <Notice class="text-sm" message={t("mobile.focusOnboarding.statusError")} copy={false} onRetry={retryStatus} />
              {/if}
              <button
                type="button"
                class={ONBOARDING_SECONDARY_BUTTON_CLASS}
                disabled={opening}
                onclick={() => (continueAfterReturn ? void openStep(step) : advance())}
              >
                {continueAfterReturn ? t("mobile.focusOnboarding.openAgain") : t("mobile.focusOnboarding.notNow")}
              </button>
            {/if}
          {/snippet}
        </OnboardingNav>
      </div>
    {/if}
  </MobileOnboardingProgress>
</main>
