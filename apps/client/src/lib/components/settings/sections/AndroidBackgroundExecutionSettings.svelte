<script lang="ts">
  import { onMount } from "svelte";
  import LoaderCircle from "@lucide/svelte/icons/loader-circle";
  import {
    mobileBackgroundExecutionStatus,
    openMobileBackgroundExecutionSettings,
    type MobileBackgroundExecutionStatus,
    type MobileBackgroundSettingsDestination,
  } from "$lib/scheduling/mobile-background-execution";
  import { getLocalization } from "$lib/i18n/translator.svelte";

  const { t } = getLocalization();
  let status = $state<MobileBackgroundExecutionStatus | null>(null);
  let loading = $state(true);
  let opening = $state<MobileBackgroundSettingsDestination | null>(null);
  let unavailable = $state(false);

  async function refresh(): Promise<void> {
    try {
      status = await mobileBackgroundExecutionStatus();
      unavailable = false;
    } catch (error) {
      console.warn("Android background execution status failed", error);
      unavailable = true;
    } finally {
      loading = false;
    }
  }

  async function open(destination: MobileBackgroundSettingsDestination): Promise<void> {
    opening = destination;
    try {
      await openMobileBackgroundExecutionSettings(destination);
    } catch (error) {
      console.warn("Android background execution settings failed", error);
    } finally {
      opening = null;
    }
  }

  onMount(() => {
    void refresh();
    const refreshWhenVisible = (): void => {
      if (document.visibilityState === "visible") void refresh();
    };
    window.addEventListener("focus", refreshWhenVisible);
    document.addEventListener("visibilitychange", refreshWhenVisible);
    return () => {
      window.removeEventListener("focus", refreshWhenVisible);
      document.removeEventListener("visibilitychange", refreshWhenVisible);
    };
  });
</script>

<div class="flex flex-col gap-3">
  {#if status?.autostartSettingsAvailable}
    <div class="flex items-center justify-between gap-4 px-1 py-1 max-[480px]:flex-col max-[480px]:items-stretch max-[480px]:gap-2">
      <div class="min-w-0 flex-1">
        <div class="text-[0.866667rem] text-foreground">{t("settings.focus.androidAutostart")}</div>
        <div class="mt-0.5 text-[0.8rem] text-muted-foreground">{t("settings.focus.androidAutostartDescription")}</div>
      </div>
      <button
        type="button"
        class="inline-flex h-7 shrink-0 items-center justify-center gap-1.5 rounded-md border border-border bg-card px-2.5 text-[0.8rem] font-medium text-foreground transition-colors hover:bg-accent disabled:cursor-wait disabled:opacity-50 max-[480px]:self-end dark:bg-transparent"
        disabled={loading || opening !== null}
        aria-busy={opening === "autostart" || undefined}
        onclick={() => void open("autostart")}
      >
        {#if opening === "autostart"}<LoaderCircle class="size-3.5 animate-spin" aria-hidden="true" />{/if}
        {t("mobile.focusOnboarding.review")}
      </button>
    </div>
  {/if}

  <div class="flex items-center justify-between gap-4 px-1 py-1 max-[480px]:flex-col max-[480px]:items-stretch max-[480px]:gap-2">
    <div class="min-w-0 flex-1">
      <div class="text-[0.866667rem] text-foreground">{t("settings.focus.androidBattery")}</div>
      <div class="mt-0.5 text-[0.8rem] text-muted-foreground">
        {status?.backgroundRestricted
          ? t("mobile.focusOnboarding.backgroundRestricted")
          : t("settings.focus.androidBatteryDescription")}
      </div>
    </div>
    <button
      type="button"
      class="inline-flex h-7 shrink-0 items-center justify-center gap-1.5 rounded-md border border-border bg-card px-2.5 text-[0.8rem] font-medium text-foreground transition-colors hover:bg-accent disabled:cursor-wait disabled:opacity-50 max-[480px]:self-end dark:bg-transparent"
      disabled={loading || opening !== null}
      aria-busy={opening === "battery" || undefined}
      onclick={() => void open("battery")}
    >
      {#if opening === "battery"}<LoaderCircle class="size-3.5 animate-spin" aria-hidden="true" />{/if}
      {t("mobile.focusOnboarding.review")}
    </button>
  </div>

  {#if unavailable}
    <p class="px-1 text-[0.8rem] text-destructive">{t("settings.focus.androidBackgroundUnavailable")}</p>
  {/if}
</div>
