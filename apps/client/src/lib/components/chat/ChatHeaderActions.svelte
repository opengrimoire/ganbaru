<script lang="ts">
  import PanelBottom from "@lucide/svelte/icons/panel-bottom";
  import PanelRight from "@lucide/svelte/icons/panel-right";
  import Settings2 from "@lucide/svelte/icons/settings-2";
  import SquareArrowOutUpRight from "@lucide/svelte/icons/square-arrow-out-up-right";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import { chatErrorMessage } from "$lib/chat/error-presentation";
  import { BUILD_PLATFORM_PROFILE, platformHasCapability } from "$lib/platform";
  import { useChat } from "$lib/stores/chat.svelte";
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

  const chat = useChat();
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
    detachBlock === "last-tab" ? t("titleBar.onlyTabInWindow") : t("chat.detach"),
  );
  /** A reply thread already shown in a tab of its own offers neither a new tab nor a new window. */
  const replyThreadTab = $derived(chat.replyThreadTabId !== null);
  const canOpenReplyThreadTab = $derived(
    !mobilePresentation && !replyThreadTab && tabs.canOpenTabs && chat.openReplyThreadId !== null && chat.selectedChannel !== null,
  );
  let actionError = $state<string | null>(null);

  function detachThread(): void {
    actionError = null;
    void tabs.detach(tabs.activeTabId).catch((error: unknown) => {
      actionError = chatErrorMessage(error);
    });
  }

  /** Moves the open reply thread out of the channel into a workspace tab of its own, right after this one. */
  function openReplyThreadInNewTab(): void {
    const channel = chat.selectedChannel;
    const replyThreadId = chat.openReplyThreadId;
    if (!channel || !replyThreadId) return;
    chat.closeReplyThread();
    tabs.openLocation("item", {
      kind: "chat",
      projectId: channel.projectId,
      item: { type: "chat-reply-thread", id: replyThreadId, channelId: channel.id },
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
  {#if canOpenReplyThreadTab}
    <button type="button" class={iconButtonClass} aria-label={t("chat.organization.openThreadInNewTab")} title={t("chat.organization.openThreadInNewTab")} data-chat-reply-thread-new-tab onclick={openReplyThreadInNewTab}>
      <SquareArrowOutUpRight size={actionIconSize} strokeWidth={1.75} />
    </button>
  {:else if localExecutionAvailable && selectedThread && !replyThreadTab && detachBlock !== "unsupported"}
    <button type="button" class={cn(iconButtonClass, detachBlock !== null && "cursor-not-allowed opacity-40")} aria-label={t("chat.detach")} aria-disabled={detachBlock !== null} title={detachTitle} onclick={() => { if (detachBlock === null) detachThread(); }}>
      <SquareArrowOutUpRight size={actionIconSize} strokeWidth={1.75} />
    </button>
  {/if}
  {#if projectName && !replyThreadTab}
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
