<script lang="ts">
  import LoaderCircle from "@lucide/svelte/icons/loader-circle";
  import { onMount } from "svelte";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import ConfirmDialog from "$lib/components/ui/ConfirmDialog.svelte";
  import Loading from "$lib/components/ui/Loading.svelte";
  import Notice from "$lib/components/ui/Notice.svelte";
  import { onboardingPrimaryAction } from "$lib/vault/onboarding-primary-action";
  import {
    mobileDistractionsAccessStatus,
    openMobileDistractionsAccessibilitySettings,
    openMobileDistractionsUsageAccessSettings,
    type MobileDistractionsAccessStatus,
  } from "$lib/scheduling/mobile-distractions";
  import {
    completeMobileFocusOnboarding,
    markMobileFocusAccessReviewed,
    mobileBackgroundExecutionStatus,
    openMobileBackgroundExecutionSettings,
    type MobileBackgroundExecutionStatus,
    type MobileBackgroundSettingsDestination,
    type MobileFocusAccessReview,
  } from "$lib/scheduling/mobile-background-execution";
  import {
    mobileNotificationAccessStatus,
    openMobileExactAlarmSettings,
    requestMobileNotificationPermission,
    resolveMobileNotificationAccess,
    type MobileNotificationAccessStatus,
  } from "$lib/scheduling/mobile-notification-access";

  let { onComplete }: { onComplete: () => void } = $props();

  const CONTINUE_DELAY_MS = 5_000;
  const { t } = getLocalization();
  const storage = safeStorage();

  let backgroundStatus = $state<MobileBackgroundExecutionStatus | null>(null);
  let notificationStatus = $state<MobileNotificationAccessStatus | null>(null);
  let distractionsStatus = $state<MobileDistractionsAccessStatus | null>(null);
  let loading = $state(true);
  let opening = $state<MobileFocusAccessReview | null>(null);
  let pendingReview = $state<MobileFocusAccessReview | null>(null);
  let leftForReview = $state(false);
  let secondsRemaining = $state(5);
  let unavailable = $state(false);
  let disclosure = $state<"accessibility" | null>(null);

  function safeStorage(): Storage | undefined {
    try {
      return window.localStorage;
    } catch {
      return undefined;
    }
  }

  function recordReviewed(review: MobileFocusAccessReview): void {
    if (!storage) return;
    try {
      markMobileFocusAccessReviewed(review, storage);
    } catch (error) {
      console.warn("Android Focus access review could not be saved", error);
    }
  }

  async function refresh(): Promise<void> {
    const [backgroundResult, notificationResult, distractionsResult] = await Promise.allSettled([
      mobileBackgroundExecutionStatus(),
      mobileNotificationAccessStatus(),
      mobileDistractionsAccessStatus(),
    ]);
    unavailable = false;
    if (backgroundResult.status === "fulfilled") {
      backgroundStatus = backgroundResult.value;
    } else {
      console.warn("Android background access status failed", backgroundResult.reason);
      unavailable = true;
    }
    if (notificationResult.status === "fulfilled") {
      notificationStatus = notificationResult.value;
    } else {
      console.warn("Android notification access status failed", notificationResult.reason);
      unavailable = true;
    }
    if (distractionsResult.status === "fulfilled") {
      distractionsStatus = distractionsResult.value;
    } else {
      console.warn("Android Distractions access status failed", distractionsResult.reason);
      unavailable = true;
    }
    loading = false;
  }

  /** Rereads every access status for the error notice and reports whether all of them are readable now. */
  async function retryStatus(): Promise<boolean> {
    await refresh();
    return !unavailable;
  }

  function beginSettingsReview(review: MobileFocusAccessReview): void {
    pendingReview = review;
    leftForReview = false;
  }

  function finishReturnedReview(): void {
    if (!pendingReview || !leftForReview) return;
    recordReviewed(pendingReview);
    pendingReview = null;
    leftForReview = false;
    void refresh();
  }

  async function openBackground(destination: MobileBackgroundSettingsDestination): Promise<void> {
    if (opening) return;
    opening = destination;
    beginSettingsReview(destination);
    try {
      await openMobileBackgroundExecutionSettings(destination);
    } catch (error) {
      pendingReview = null;
      console.warn("Android background settings failed", error);
      unavailable = true;
    } finally {
      opening = null;
    }
  }

  async function reviewNotifications(): Promise<void> {
    if (opening || !notificationStatus) return;
    opening = "notifications";
    try {
      if (notificationStatus.permission === "prompt") {
        notificationStatus = await requestMobileNotificationPermission();
        recordReviewed("notifications");
      } else {
        beginSettingsReview("notifications");
        await resolveMobileNotificationAccess(notificationStatus);
      }
    } catch (error) {
      pendingReview = null;
      console.warn("Android notification access failed", error);
      unavailable = true;
    } finally {
      opening = null;
    }
  }

  async function reviewExactAlarm(): Promise<void> {
    if (opening) return;
    opening = "exact-alarm";
    beginSettingsReview("exact-alarm");
    try {
      await openMobileExactAlarmSettings();
    } catch (error) {
      pendingReview = null;
      console.warn("Android exact-alarm settings failed", error);
      unavailable = true;
    } finally {
      opening = null;
    }
  }

  function reviewDistractions(target: "usage" | "accessibility"): void {
    if (target === "accessibility" && !distractionsStatus?.accessibility) {
      disclosure = target;
      return;
    }
    void openDistractionsSettings(target);
  }

  async function openDistractionsSettings(
    target: "usage" | "accessibility" | null,
  ): Promise<void> {
    if (!target || opening) return;
    const review: MobileFocusAccessReview = target === "usage" ? "usage-access" : "app-blocking";
    opening = review;
    beginSettingsReview(review);
    try {
      if (target === "usage") await openMobileDistractionsUsageAccessSettings();
      else await openMobileDistractionsAccessibilitySettings();
    } catch (error) {
      pendingReview = null;
      console.warn("Android Distractions settings failed", error);
      unavailable = true;
    } finally {
      opening = null;
    }
  }

  function agreeAndReviewDistractions(): void {
    const target = disclosure;
    disclosure = null;
    void openDistractionsSettings(target);
  }

  function complete(): void {
    if (secondsRemaining > 0) return;
    if (storage) {
      try {
        completeMobileFocusOnboarding(storage);
      } catch (error) {
        console.warn("Android Focus onboarding completion could not be saved", error);
      }
    }
    onComplete();
  }

  onMount(() => {
    const readyAt = Date.now() + CONTINUE_DELAY_MS;
    const updateCountdown = (): void => {
      secondsRemaining = Math.max(0, Math.ceil((readyAt - Date.now()) / 1_000));
    };
    const countdown = window.setInterval(updateCountdown, 200);
    const handleVisibility = (): void => {
      if (document.visibilityState === "hidden" && pendingReview) {
        leftForReview = true;
      } else if (document.visibilityState === "visible") {
        updateCountdown();
        finishReturnedReview();
      }
    };
    const handleBlur = (): void => {
      if (pendingReview) leftForReview = true;
    };
    const handleFocus = (): void => {
      updateCountdown();
      finishReturnedReview();
    };

    updateCountdown();
    void refresh();
    document.addEventListener("visibilitychange", handleVisibility);
    window.addEventListener("blur", handleBlur);
    window.addEventListener("focus", handleFocus);

    return () => {
      window.clearInterval(countdown);
      document.removeEventListener("visibilitychange", handleVisibility);
      window.removeEventListener("blur", handleBlur);
      window.removeEventListener("focus", handleFocus);
    };
  });
</script>

{#snippet accessRow(
  review: MobileFocusAccessReview,
  title: string,
  description: string,
  action: () => void,
)}
  <div class="grid grid-cols-[minmax(0,1fr)_auto] items-center gap-3 py-3.5">
    <div class="min-w-0">
      <div class="text-sm font-medium text-foreground">{title}</div>
      <p class="mt-1 text-sm leading-5 text-muted-foreground">{description}</p>
    </div>
    <div class="flex items-center justify-end">
      <button
        type="button"
        class="inline-flex h-9 shrink-0 items-center gap-1.5 rounded-md bg-primary px-3 text-sm font-medium text-primary-foreground transition-colors active:bg-primary/90 disabled:cursor-wait disabled:opacity-50"
        disabled={opening !== null}
        aria-busy={opening === review || undefined}
        onclick={action}
      >
        {#if opening === review}<LoaderCircle class="size-3.5 animate-spin" aria-hidden="true" />{/if}
        {t("mobile.focusOnboarding.review")}
      </button>
    </div>
  </div>
{/snippet}

<main
  class="mobile-viewport-height w-screen overflow-hidden bg-background pb-(--safe-area-bottom) pt-(--safe-area-top) text-foreground"
  style="padding-left: var(--safe-area-left); padding-right: var(--safe-area-right);"
>
  <section class="h-full overflow-y-auto px-4 min-[560px]:px-8 min-[760px]:px-10">
    <div class="focus-setup-grid mx-auto grid min-h-full w-full max-w-2xl py-5 min-[760px]:py-8">
      <div></div>

      <div class="flex flex-col gap-6">
        <div class="space-y-2">
          <h1 class="max-w-xl text-2xl font-semibold leading-tight text-foreground min-[560px]:text-3xl">
            {t("mobile.focusOnboarding.title")}
          </h1>
          <p class="text-sm leading-6 text-muted-foreground">
            {t("mobile.focusOnboarding.description")}
          </p>
        </div>

        <div class="divide-y divide-border border-y border-border">
          {#if notificationStatus && notificationStatus.permission !== "granted"}
            {@render accessRow(
              "notifications",
              t("mobile.focusOnboarding.notifications"),
              t("mobile.focusOnboarding.notificationsDescription"),
              () => { void reviewNotifications(); },
            )}
          {/if}

          {#if notificationStatus?.exactAlarm.required && !notificationStatus.exactAlarm.granted}
            {@render accessRow(
              "exact-alarm",
              t("mobile.focusOnboarding.exactAlarm"),
              t("mobile.focusOnboarding.exactAlarmDescription"),
              () => { void reviewExactAlarm(); },
            )}
          {/if}

          {#if backgroundStatus?.autostartSettingsAvailable}
            {@render accessRow(
              "autostart",
              t("settings.focus.androidAutostart"),
              t("settings.focus.androidAutostartDescription"),
              () => { void openBackground("autostart"); },
            )}
          {/if}

          {@render accessRow(
            "battery",
            t("settings.focus.androidBattery"),
            backgroundStatus?.backgroundRestricted
              ? t("mobile.focusOnboarding.backgroundRestricted")
              : t("settings.focus.androidBatteryDescription"),
            () => { void openBackground("battery"); },
          )}

          {#if distractionsStatus}
            {@render accessRow(
              "usage-access",
              t("mobile.focusOnboarding.usageAccess"),
              t("mobile.focusOnboarding.usageAccessDescription"),
              () => reviewDistractions("usage"),
            )}

            {@render accessRow(
              "app-blocking",
              t("mobile.focusOnboarding.appBlocking"),
              t("mobile.focusOnboarding.appBlockingDescription"),
              () => reviewDistractions("accessibility"),
            )}
          {/if}
        </div>

        <button
          use:onboardingPrimaryAction
          type="button"
          class="flex min-h-11 w-full items-center justify-center gap-2 rounded-md bg-primary px-3 py-2 text-sm font-medium text-primary-foreground transition-colors active:bg-primary/90 disabled:cursor-not-allowed disabled:opacity-60"
          disabled={secondsRemaining > 0}
          onclick={complete}
        >
          {secondsRemaining > 0
            ? t("mobile.focusOnboarding.continueIn", secondsRemaining)
            : t("mobile.focusOnboarding.continue")}
        </button>
      </div>

      <div class="pt-4">
        {#if loading}
          <div class="flex justify-center text-sm"><Loading layout="inline" showLabel label={t("mobile.focusOnboarding.checkingAccess")} /></div>
        {:else if unavailable}
          <Notice class="text-sm" message={t("mobile.focusOnboarding.statusError")} copy={false} onRetry={retryStatus} />
        {/if}
      </div>
    </div>
  </section>
</main>

{#if disclosure}
  <ConfirmDialog
    title={t("settings.distractions.mobile.disclosureBlockingTitle")}
    message={t("settings.distractions.mobile.disclosureBlockingMessage")}
    confirmLabel={t("settings.distractions.mobile.agreeAndReview")}
    cancelLabel={t("settings.distractions.mobile.notNow")}
    onConfirm={agreeAndReviewDistractions}
    onCancel={() => { disclosure = null; }}
  />
{/if}

<style>
  .focus-setup-grid {
    grid-template-rows: minmax(3rem, 1fr) auto minmax(3rem, 1fr);
  }
</style>
