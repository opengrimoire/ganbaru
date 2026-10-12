<script lang="ts">
  import { onMount } from "svelte";
  import { cn } from "$lib/utils";
  import ConfirmDialog from "$lib/components/ui/ConfirmDialog.svelte";
  import Notice from "$lib/components/ui/Notice.svelte";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import { getDistractions } from "$lib/stores/distractions.svelte";
  import {
    mobileDistractionsAccessStatus,
    openMobileDistractionsAccessibilitySettings,
    openMobileDistractionsUsageAccessSettings,
    type MobileDistractionsAccessStatus,
  } from "$lib/scheduling/mobile-distractions";
  import DistractionsConfigurationSection from "./DistractionsConfigurationSection.svelte";
  import DistractionsMobileAppSelector, {
    type DistractionsMobileAppSelection,
  } from "./DistractionsMobileAppSelector.svelte";
  import DistractionsRuleList from "./DistractionsRuleList.svelte";

  type ConfigurationToggle = "enabled" | "focus" | "shortBreaks" | "longBreaks" | "pause";
  type AccessTarget = "usage" | "accessibility";
  type PendingAction =
    | { target: "configuration"; toggle: ConfigurationToggle }
    | { target: "app"; type: "disable" | "delete"; packageName: string; name: string };

  const distractions = getDistractions();
  const { t } = getLocalization();
  let status = $state<MobileDistractionsAccessStatus | null>(null);
  let loadingStatus = $state(true);
  let statusError = $state(false);
  let pickerOpen = $state(false);
  let disclosure = $state<"accessibility" | null>(null);
  let pendingAction = $state<PendingAction | null>(null);

  const protectionActive = $derived(Boolean(status?.usageAccess && status.accessibility));
  const appItems = $derived(distractions.blockedMobileApps.map((rule) => ({
    id: rule.packageName,
    label: rule.name,
    enabled: rule.enabled,
  })));

  async function refreshStatus(): Promise<void> {
    try {
      status = await mobileDistractionsAccessStatus();
      statusError = false;
    } catch (error) {
      console.warn("Android Distractions access status failed", error);
      statusError = true;
    } finally {
      loadingStatus = false;
    }
  }

  /**
   * Reloads Android access status for the error notice retry action.
   *
   * @returns A promise that rejects when the status still cannot be read.
   */
  async function retryStatus(): Promise<void> {
    await refreshStatus();
    if (statusError) throw new Error("Android Distractions access status is unavailable");
  }

  async function openAccessSettings(target: AccessTarget | null): Promise<void> {
    try {
      if (target === "usage") {
        await openMobileDistractionsUsageAccessSettings();
      } else if (target === "accessibility") {
        await openMobileDistractionsAccessibilitySettings();
      }
    } catch (error) {
      console.warn("Android Distractions settings failed", error);
      statusError = true;
    }
  }

  function reviewAccess(target: AccessTarget): void {
    if (target === "accessibility" && !status?.accessibility) {
      disclosure = target;
      return;
    }
    void openAccessSettings(target);
  }

  function agreeAndReview(): void {
    const target = disclosure;
    disclosure = null;
    void openAccessSettings(target);
  }

  function setConfiguration(toggle: ConfigurationToggle, checked: boolean): void {
    if (toggle === "enabled") distractions.setMobileEnabled(checked);
    else if (toggle === "focus") distractions.setMobileBlockDuringFocus(checked);
    else if (toggle === "shortBreaks") distractions.setMobileBlockDuringShortBreaks(checked);
    else if (toggle === "longBreaks") distractions.setMobileBlockDuringLongBreaks(checked);
    else distractions.setMobilePauseDuringFocusPause(checked);
  }

  function requestConfigurationChange(toggle: ConfigurationToggle, checked: boolean): void {
    if (checked) setConfiguration(toggle, true);
    else pendingAction = { target: "configuration", toggle };
  }

  function requestAppChange(packageName: string, enabled: boolean): void {
    const app = distractions.blockedMobileApps.find((rule) => rule.packageName === packageName);
    if (!app) return;
    if (enabled) distractions.setBlockedMobileAppEnabled(packageName, true);
    else pendingAction = { target: "app", type: "disable", packageName, name: app.name };
  }

  function requestAppDelete(packageName: string): void {
    const app = distractions.blockedMobileApps.find((rule) => rule.packageName === packageName);
    if (!app) return;
    pendingAction = { target: "app", type: "delete", packageName, name: app.name };
  }

  function confirmPendingAction(): void {
    if (!pendingAction) return;
    if (pendingAction.target === "configuration") {
      setConfiguration(pendingAction.toggle, false);
    } else if (pendingAction.type === "disable") {
      distractions.setBlockedMobileAppEnabled(pendingAction.packageName, false);
    } else {
      distractions.removeBlockedMobileApp(pendingAction.packageName);
    }
    pendingAction = null;
  }

  function pendingTitle(action: PendingAction): string {
    if (action.target === "app") {
      return action.type === "disable"
        ? t("settings.distractions.mobile.allowAppTitle", action.name)
        : t("settings.distractions.mobile.removeAppTitle", action.name);
    }
    if (action.toggle === "enabled") return t("settings.distractions.mobile.turnOffTitle");
    if (action.toggle === "focus") return t("settings.distractions.mobile.allowAppsFocusTitle");
    if (action.toggle === "shortBreaks") return t("settings.distractions.mobile.allowAppsShortBreaksTitle");
    if (action.toggle === "longBreaks") return t("settings.distractions.mobile.allowAppsLongBreaksTitle");
    return t("settings.distractions.mobile.keepBlockingPausedTitle");
  }

  function pendingMessage(action: PendingAction): string {
    if (action.target === "app") {
      return action.type === "disable"
        ? t("settings.distractions.mobile.appDisableMessage")
        : t("settings.distractions.mobile.removeMessage");
    }
    if (action.toggle === "enabled") return t("settings.distractions.mobile.appOffMessage");
    if (action.toggle === "focus") return t("settings.distractions.mobile.focusOffMessage");
    if (action.toggle === "shortBreaks") return t("settings.distractions.mobile.shortBreaksOffMessage");
    if (action.toggle === "longBreaks") return t("settings.distractions.mobile.longBreaksOffMessage");
    return t("settings.distractions.mobile.pauseActiveMessage");
  }

  onMount(() => {
    void refreshStatus();
    const refresh = (): void => {
      if (document.visibilityState === "visible") void refreshStatus();
    };
    window.addEventListener("focus", refresh);
    document.addEventListener("visibilitychange", refresh);
    return () => {
      window.removeEventListener("focus", refresh);
      document.removeEventListener("visibilitychange", refresh);
    };
  });
</script>

<div class="flex flex-col gap-6">
  <DistractionsConfigurationSection
    title={t("settings.distractions.mobile.mobileConfiguration")}
    enabled={distractions.mobileEnabled}
    blockDuringFocus={distractions.mobileBlockDuringFocus}
    blockDuringShortBreaks={distractions.mobileBlockDuringShortBreaks}
    blockDuringLongBreaks={distractions.mobileBlockDuringLongBreaks}
    pauseDuringFocusPause={distractions.mobilePauseDuringFocusPause}
    showMode={false}
    enabledLabel={t("settings.distractions.mobile.enableMobileBlocking")}
    enabledDescription={t("settings.distractions.mobile.enableMobileBlockingDescription")}
    focusDescription={t("settings.distractions.mobile.focusDescription")}
    shortBreakDescription={t("settings.distractions.mobile.shortBreakDescription")}
    longBreakDescription={t("settings.distractions.mobile.longBreakDescription")}
    pauseDescription={t("settings.distractions.mobile.pauseDescription")}
    onScheduleChange={requestConfigurationChange}
  />

  <div class="h-px shrink-0 scale-y-50 bg-border" aria-hidden="true"></div>

  <section class="flex flex-col gap-3">
    <h2 class="px-1 text-[0.866667rem] font-semibold text-foreground">{t("settings.distractions.mobile.appBlocking")}</h2>
    <div class="flex flex-col gap-3">
      <div class="flex items-center justify-between gap-4 px-1 py-1">
        <div class="min-w-0 flex-1">
          <div class="text-[0.866667rem] text-foreground">{t("settings.distractions.mobile.usageAccess")}</div>
          <div class="mt-0.5 text-[0.8rem] text-muted-foreground">{t("settings.distractions.mobile.usageAccessDescription")}</div>
        </div>
        <button type="button" onclick={() => reviewAccess("usage")} class="h-7 shrink-0 rounded-md bg-primary px-2.5 text-[0.8rem] font-medium text-primary-foreground hover:bg-primary/90">{t("mobile.focusOnboarding.review")}</button>
      </div>
      <div class="flex items-center justify-between gap-4 px-1 py-1">
        <div class="min-w-0 flex-1">
          <div class="text-[0.866667rem] text-foreground">{t("settings.distractions.mobile.appBlocking")}</div>
          <div class="mt-0.5 text-[0.8rem] text-muted-foreground">{t("settings.distractions.mobile.appBlockingDescription")}</div>
        </div>
        <button type="button" onclick={() => reviewAccess("accessibility")} class="h-7 shrink-0 rounded-md bg-primary px-2.5 text-[0.8rem] font-medium text-primary-foreground hover:bg-primary/90">{t("mobile.focusOnboarding.review")}</button>
      </div>
    </div>
    <p class={cn("px-1 text-[0.8rem]", protectionActive ? "text-muted-foreground" : "text-destructive")}>
      {loadingStatus
        ? t("common.loading")
        : protectionActive
          ? t("settings.distractions.mobile.protectionActive")
          : t("settings.distractions.mobile.protectionInactive")}
    </p>
    {#if statusError}<Notice class="px-1 text-[0.8rem]" message={t("mobile.focusOnboarding.statusError")} onRetry={retryStatus} />{/if}
  </section>

  <fieldset disabled={!distractions.mobileEnabled} class={cn("m-0 flex min-w-0 flex-col gap-4 border-0 p-0", !distractions.mobileEnabled && "opacity-50")}>
    <div class="h-px shrink-0 scale-y-50 bg-border" aria-hidden="true"></div>
    <section class="flex flex-col gap-4">
      <h2 class="px-1 text-[0.866667rem] font-semibold text-foreground">{t("settings.distractions.mobile.blocklist")}</h2>
      <DistractionsRuleList
        id="distractions-mobile-blocked-apps"
        heading={t("settings.distractions.mobile.blockedApps")}
        description={t("settings.distractions.mobile.blockedAppsDescription")}
        placeholder=""
        emptyText={t("settings.distractions.mobile.noBlockedApps")}
        errorText=""
        items={appItems}
        onAdd={() => false}
        onOpenSelector={() => { pickerOpen = true; }}
        selectorLabel={t("settings.distractions.mobile.addApp")}
        onEnabledChange={requestAppChange}
        onDelete={requestAppDelete}
      />
    </section>
  </fieldset>
</div>

{#if pickerOpen}
  <DistractionsMobileAppSelector
    title={t("settings.distractions.mobile.chooseAppToBlock")}
    existingPackages={distractions.blockedMobileApps.map((rule) => rule.packageName)}
    onAdd={(app: DistractionsMobileAppSelection) => distractions.addBlockedMobileApp(app.name, app.packageName)}
    onRemove={(packageName) => distractions.removeBlockedMobileApp(packageName)}
    onCancel={() => { pickerOpen = false; }}
  />
{/if}

{#if disclosure}
  <ConfirmDialog
    title={t("settings.distractions.mobile.disclosureBlockingTitle")}
    message={t("settings.distractions.mobile.disclosureBlockingMessage")}
    confirmLabel={t("settings.distractions.mobile.agreeAndReview")}
    cancelLabel={t("settings.distractions.mobile.notNow")}
    onConfirm={agreeAndReview}
    onCancel={() => { disclosure = null; }}
  />
{/if}

{#if pendingAction}
  <ConfirmDialog
    title={pendingTitle(pendingAction)}
    message={pendingMessage(pendingAction)}
    confirmLabel={pendingAction.target === "app" && pendingAction.type === "delete" ? t("settings.distractions.shared.removeAction") : t("settings.distractions.shared.allowAction")}
    cancelLabel={t("settings.distractions.shared.cancelAction")}
    onConfirm={confirmPendingAction}
    onCancel={() => { pendingAction = null; }}
  />
{/if}
