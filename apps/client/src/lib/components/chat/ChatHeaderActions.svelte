<script lang="ts">
  import PanelBottom from "@lucide/svelte/icons/panel-bottom";
  import PanelRight from "@lucide/svelte/icons/panel-right";
  import Settings2 from "@lucide/svelte/icons/settings-2";
  import SquareArrowOutUpRight from "@lucide/svelte/icons/square-arrow-out-up-right";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import { BUILD_PLATFORM_PROFILE, platformHasCapability } from "$lib/platform";
  import { getChat } from "$lib/stores/chat.svelte";
  import { getWorkspaceTabs } from "$lib/stores/workspace-tabs.svelte";
  import { cn } from "$lib/utils";
  import Notice from "$lib/components/ui/Notice.svelte";

  let {
    bottomPanelOpen,
    projectName,
    projectSettingsOpen,
    onToggleBottomPanel,
    onToggleProjectSettings,
  }: {
    bottomPanelOpen: boolean;
    projectName: string | null;
    projectSettingsOpen: boolean;
    onToggleBottomPanel: () => void;
    onToggleProjectSettings: (trigger: HTMLButtonElement) => void;
  } = $props();

  const chat = getChat();
  const { t } = getLocalization();
  const localExecutionAvailable = platformHasCapability(
    BUILD_PLATFORM_PROFILE,
    "chat.local-execution",
  );
  const mobilePresentation = BUILD_PLATFORM_PROFILE.shell === "mobile";
  /** Phone action rows share 16 pixel icons in 3rem tall targets on the top bar icon columns across Projects, Notes, and Chat. */
  const actionIconSize = mobilePresentation ? 16 : 14;
  const iconButtonClass = cn("chat-header-icon-button", mobilePresentation && "mobile-chrome-button");
  const tabs = getWorkspaceTabs();
  const selectedThread = $derived(chat.selectedThread);
  const detachBlock = $derived(tabs.detachBlock(tabs.activeTabId));
  const detachTitle = $derived(
    detachBlock === "last-tab"
      ? t("titleBar.keepOneTabInMainWindow")
      : detachBlock === "kind-has-other-tabs"
        ? t("titleBar.tab.kindHasOtherTabs", t("titleBar.tab.chat"))
        : t("chat.detach"),
  );
  let actionError = $state<string | null>(null);

  function detachThread(): void {
    actionError = null;
    void tabs.detach(tabs.activeTabId).catch((error: unknown) => {
      actionError = error instanceof Error ? error.message : String(error);
    });
  }
</script>

<div class={cn("chat-header-actions flex min-w-0 shrink-0 items-center", mobilePresentation ? "gap-0" : "gap-1")} class:mobilePresentation data-chat-header-actions>
  {#if actionError}<Notice class="header-action-error max-w-48 text-[0.666667rem]" message={actionError} />{/if}
  {#if localExecutionAvailable}
    <button type="button" class={cn(iconButtonClass, bottomPanelOpen && "bg-accent text-foreground")} aria-label={bottomPanelOpen ? t("chat.closeBottomPanel") : t("chat.openBottomPanel")} aria-pressed={bottomPanelOpen} data-chat-bottom-panel-action onclick={onToggleBottomPanel}>
      <PanelBottom size={actionIconSize} strokeWidth={1.75} />
    </button>
    <button type="button" class={cn(iconButtonClass, chat.inspectorOpen && "bg-accent text-foreground")} aria-label={chat.inspectorOpen ? t("chat.closeInspector") : t("chat.openInspector")} aria-pressed={chat.inspectorOpen} data-chat-inspector-action onclick={() => { chat.inspectorOpen = !chat.inspectorOpen; }}>
      <PanelRight size={actionIconSize} strokeWidth={1.75} />
    </button>
  {/if}
  {#if localExecutionAvailable && selectedThread && detachBlock !== "unsupported"}
    <button type="button" class={cn(iconButtonClass, detachBlock !== null && "cursor-not-allowed opacity-40")} aria-label={t("chat.detach")} aria-disabled={detachBlock !== null} title={detachTitle} onclick={() => { if (detachBlock === null) detachThread(); }}>
      <SquareArrowOutUpRight size={actionIconSize} strokeWidth={1.75} />
    </button>
  {/if}
  {#if projectName}
    <button
      type="button"
      class={cn(iconButtonClass, projectSettingsOpen && "bg-accent text-foreground")}
      aria-label={t("chat.projectSettingsTitle", projectName)}
      title={t("chat.projectSettingsTitle", projectName)}
      aria-haspopup="dialog"
      aria-expanded={projectSettingsOpen}
      data-chat-project-settings-trigger
      onclick={(event) => onToggleProjectSettings(event.currentTarget)}
    >
      <Settings2 size={actionIconSize} strokeWidth={1.75} />
    </button>
  {/if}
</div>

<style>
  .chat-header-icon-button { display: flex; flex: 0 0 auto; align-items: center; justify-content: center; color: var(--foreground); transition: background-color 120ms ease; }
  .chat-header-actions:not(.mobilePresentation) .chat-header-icon-button { height: 1.75rem; width: 1.75rem; border-radius: 0.375rem; }
  @media (hover: hover) { .chat-header-icon-button:hover { background-color: var(--accent); } }
  .chat-header-actions :global(.header-action-error .notice-message) { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .chat-header-actions.mobilePresentation .chat-header-icon-button { width: var(--mobile-chrome-column); }
  @media (prefers-reduced-motion: reduce) { .chat-header-icon-button { transition: none; } }
</style>
