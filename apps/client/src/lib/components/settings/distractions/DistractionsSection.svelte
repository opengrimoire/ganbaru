<script lang="ts">
  import { cn } from "$lib/utils";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import type { DistractionsLimitEditorTarget, DistractionsSettingsTab } from "$lib/settings/types";
  import DistractionsBrowserSettings from "./DistractionsBrowserSettings.svelte";
  import DistractionsMobileSettings from "./DistractionsMobileSettings.svelte";
  import DistractionsDesktopSettings from "./DistractionsDesktopSettings.svelte";
  import DistractionsLimitsSettings from "./DistractionsLimitsSettings.svelte";

  let {
    initialTab = "limits",
    onOpenLimitEditor = () => {},
  }: {
    initialTab?: DistractionsSettingsTab;
    onOpenLimitEditor?: (target: DistractionsLimitEditorTarget) => void;
  } = $props();

  const { t } = getLocalization();
  const isAndroid = __GANBARU_AI_BUILD_PLATFORM__ === "android";

  const tabs: ReadonlyArray<{
    id: DistractionsSettingsTab;
    label: () => string;
  }> = [
    { id: "limits", label: () => t("settings.distractions.tab.limits") },
    { id: "browser", label: () => t("settings.distractions.tab.browser") },
    { id: "mobile", label: () => t("settings.distractions.tab.mobile") },
    { id: "desktop", label: () => t("settings.distractions.tab.desktop") },
  ];

  let activeTab = $state<DistractionsSettingsTab>("limits");

  $effect.pre(() => {
    activeTab = initialTab;
  });
</script>

<div class="flex flex-col gap-6">
  <div
    class="grid grid-cols-2 gap-1 rounded-md border border-border bg-card p-1 min-[500px]:grid-cols-4 dark:bg-transparent"
    role="tablist"
    aria-label={t("settings.distractions.tabLabel")}
  >
    {#each tabs as tab}
      {@const active = activeTab === tab.id}
      <button
        type="button"
        role="tab"
        aria-selected={active}
        class={cn(
          "flex min-h-8 items-center justify-center rounded-sm px-2.5 text-center text-[0.8rem] font-medium text-muted-foreground",
          active && "bg-background text-foreground dark:bg-foreground/5",
        )}
        onclick={() => {
          activeTab = tab.id;
        }}
      >
        {tab.label()}
      </button>
    {/each}
  </div>

  {#if activeTab === "limits"}
    <DistractionsLimitsSettings {onOpenLimitEditor} />
  {:else if activeTab === "browser"}
    {#if isAndroid}
      <p class="px-1 text-[0.8rem] text-muted-foreground">{t("settings.distractions.mobile.browserReadOnly")}</p>
      <fieldset disabled class="m-0 min-w-0 border-0 p-0 opacity-60">
        <DistractionsBrowserSettings showConnectionStatus={false} />
      </fieldset>
    {:else}
      <DistractionsBrowserSettings />
    {/if}
  {:else if activeTab === "mobile"}
    <DistractionsMobileSettings />
  {:else}
    {#if isAndroid}
      <p class="px-1 text-[0.8rem] text-muted-foreground">{t("settings.distractions.mobile.desktopReadOnly")}</p>
      <fieldset disabled class="m-0 min-w-0 border-0 p-0 opacity-60">
        <DistractionsDesktopSettings />
      </fieldset>
    {:else}
      <DistractionsDesktopSettings />
    {/if}
  {/if}
</div>
