<script lang="ts">
  import PanelBottom from "@lucide/svelte/icons/panel-bottom";
  import PanelRight from "@lucide/svelte/icons/panel-right";
  import Settings2 from "@lucide/svelte/icons/settings-2";
  import SquareArrowOutUpRight from "@lucide/svelte/icons/square-arrow-out-up-right";
  import { openDetachedChatWindow } from "$lib/chat/workspace/local-execution-ui";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import { BUILD_PLATFORM_PROFILE, platformHasCapability } from "$lib/platform";
  import { getChat } from "$lib/stores/chat.svelte";
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
  const selectedThread = $derived(chat.selectedThread);
  let actionError = $state<string | null>(null);

  function detachThread(): void {
    actionError = null;
    void openDetachedChatWindow().catch((error: unknown) => {
      actionError = error instanceof Error ? error.message : String(error);
    });
  }
</script>

<div class={cn("chat-header-actions flex min-w-0 shrink-0 items-center", mobilePresentation ? "gap-0" : "gap-1")} class:mobilePresentation data-chat-header-actions>
  {#if actionError}<Notice class="header-action-error max-w-48 text-[0.666667rem]" message={actionError} />{/if}
  {#if localExecutionAvailable}
    <button type="button" class={cn("chat-header-icon-button", bottomPanelOpen && "bg-accent text-foreground")} aria-label={bottomPanelOpen ? t("chat.closeBottomPanel") : t("chat.openBottomPanel")} aria-pressed={bottomPanelOpen} data-chat-bottom-panel-action onclick={onToggleBottomPanel}>
      <PanelBottom size={actionIconSize} strokeWidth={1.75} />
    </button>
    <button type="button" class={cn("chat-header-icon-button", chat.inspectorOpen && "bg-accent text-foreground")} aria-label={chat.inspectorOpen ? t("chat.closeInspector") : t("chat.openInspector")} aria-pressed={chat.inspectorOpen} data-chat-inspector-action onclick={() => { chat.inspectorOpen = !chat.inspectorOpen; }}>
      <PanelRight size={actionIconSize} strokeWidth={1.75} />
    </button>
  {/if}
  {#if localExecutionAvailable && selectedThread}
    <button type="button" class="chat-header-icon-button" aria-label={t("chat.detach")} title={t("chat.detach")} onclick={detachThread}>
      <SquareArrowOutUpRight size={actionIconSize} strokeWidth={1.75} />
    </button>
  {/if}
  {#if projectName}
    <button
      type="button"
      class={cn("chat-header-icon-button", projectSettingsOpen && "bg-accent text-foreground")}
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
  .chat-header-icon-button { display: flex; height: 1.75rem; width: 1.75rem; flex: 0 0 auto; align-items: center; justify-content: center; border-radius: 0.375rem; color: var(--foreground); transition: background-color 120ms ease; }
  .chat-header-icon-button:hover { background: var(--accent); }
  .chat-header-actions :global(.header-action-error .notice-message) { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .chat-header-actions.mobilePresentation .chat-header-icon-button { width: var(--mobile-chrome-column); height: 3rem; }
  @media (prefers-reduced-motion: reduce) { .chat-header-icon-button { transition: none; } }
</style>
