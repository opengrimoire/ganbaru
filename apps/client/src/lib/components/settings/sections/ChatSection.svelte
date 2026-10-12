<script lang="ts">
  import { onMount } from "svelte";
  import { chatErrorMessage } from "$lib/chat/error-presentation";
  import { cn } from "$lib/utils";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import { getChat } from "$lib/stores/chat.svelte";
  import type { ChatProviderSetupTarget, ChatSettingsSubsection } from "$lib/settings/types";
  import ChatBehaviorSettings from "$lib/components/settings/chat/ChatBehaviorSettings.svelte";
  import ChatPermissionsSettings from "$lib/components/settings/chat/ChatPermissionsSettings.svelte";
  import ChatProvidersSettings from "$lib/components/settings/chat/ChatProvidersSettings.svelte";
  import ChatTeammatesSettings from "$lib/components/settings/chat/ChatTeammatesSettings.svelte";
  import Loading from "$lib/components/ui/Loading.svelte";
  import Notice from "$lib/components/ui/Notice.svelte";

  let {
    initialSubsection,
    initialChatTeammateId,
    initialChatChannelId,
    initialChatCreateTeammate,
    onOpenProviderSetup = () => {},
    onSubsectionChange = () => {},
    onRequestNavigation = (navigate) => navigate(),
    onTeammateDraftStateChange = () => {},
  }: {
    initialSubsection?: ChatSettingsSubsection;
    initialChatTeammateId?: string;
    initialChatChannelId?: string;
    initialChatCreateTeammate?: boolean;
    onOpenProviderSetup?: (target: ChatProviderSetupTarget) => void;
    onSubsectionChange?: (subsection: ChatSettingsSubsection) => void;
    onRequestNavigation?: (navigate: () => void) => void;
    onTeammateDraftStateChange?: (open: boolean) => void;
  } = $props();

  const { t } = getLocalization();
  const chat = getChat();
  let activeTab = $state<ChatSettingsSubsection>("teammates");
  let initializing = $state(!chat.loaded);
  let loadError = $state<string | null>(null);
  const tabs: ReadonlyArray<{
    id: ChatSettingsSubsection;
    label: () => string;
  }> = [
    { id: "teammates", label: () => t("settings.chat.teammates.heading") },
    { id: "providers", label: () => t("settings.chat.providers.heading") },
    { id: "permissions", label: () => t("settings.chat.permissions.heading") },
    { id: "behavior", label: () => t("settings.chat.behavior.heading") },
  ];

  $effect.pre(() => {
    if (initialSubsection) activeTab = initialSubsection;
  });

  onMount(() => {
    void initialize();
  });

  async function initialize(): Promise<void> {
    initializing = !chat.loaded;
    loadError = null;
    try {
      await chat.ensureLoaded();
    } catch (error: unknown) {
      loadError = chatErrorMessage(error, t("settings.chat.loadFailed"));
    } finally {
      initializing = false;
    }
  }

  /** Retries loading Chat settings and rejects when the load fails again. */
  async function retryInitialize(): Promise<void> {
    await initialize();
    if (loadError) throw new Error(loadError);
  }

  function selectTab(tab: ChatSettingsSubsection): void {
    if (tab === activeTab) return;
    onRequestNavigation(() => {
      activeTab = tab;
      onSubsectionChange(tab);
    });
  }
</script>

<div
  class={cn(
    activeTab === "teammates" || activeTab === "providers"
      ? "grid h-full min-h-0 grid-rows-[auto_minmax(0,1fr)] gap-6"
      : "flex flex-col gap-6 pb-4",
  )}
>
  <div
    class="grid grid-cols-2 gap-1 rounded-md border border-border bg-card p-1 min-[560px]:grid-cols-4 dark:bg-transparent"
    role="tablist"
    aria-label={t("settings.chat.tabLabel")}
  >
    {#each tabs as tab (tab.id)}
      {@const active = activeTab === tab.id}
      <button
        type="button"
        role="tab"
        aria-selected={active}
        class={cn(
          "flex min-h-8 items-center justify-center rounded-sm px-2 text-center text-[0.8rem] font-medium text-muted-foreground",
          active && "bg-background text-foreground dark:bg-foreground/5",
        )}
        onclick={() => selectTab(tab.id)}
      >
        <span>{tab.label()}</span>
      </button>
    {/each}
  </div>

  {#if initializing}
    <Loading layout="fill" class="min-h-40" />
  {:else if loadError}
    <div class="min-h-40 text-sm"><Notice layout="fill" message={loadError} onRetry={retryInitialize} /></div>
  {:else if activeTab === "teammates"}
    <ChatTeammatesSettings initialTeammateId={initialChatTeammateId} initialChannelId={initialChatChannelId} initialCreate={initialChatCreateTeammate} onDraftStateChange={onTeammateDraftStateChange} />
  {:else if activeTab === "providers"}
    <ChatProvidersSettings {onOpenProviderSetup} />
  {:else if activeTab === "permissions"}
    <div data-chat-settings-subsection="permissions"><ChatPermissionsSettings /></div>
  {:else}
    <div data-chat-settings-subsection="behavior"><ChatBehaviorSettings /></div>
  {/if}
</div>

<style>
  :global(.chat-settings-button) { display: inline-flex; min-height: 2rem; align-items: center; justify-content: center; gap: 0.375rem; border-radius: 0.375rem; border: 1px solid var(--border); background: var(--background); padding: 0.25rem 0.625rem; font-size: calc(0.733333rem * var(--type-scale)); font-weight: 500; color: var(--foreground); }
  :global(.chat-settings-button:hover:not(:disabled)) { background: var(--accent); }
  :global(.chat-settings-button:disabled) { cursor: not-allowed; opacity: 0.5; }
  :global(.setup-field), :global(.chat-inline-field) { display: flex; flex-direction: column; gap: 0.375rem; font-size: calc(0.733333rem * var(--type-scale)); font-weight: 500; color: var(--muted-foreground); }
  :global(.setup-field input), :global(.chat-inline-field input) { min-height: 2.25rem; border-radius: var(--floating-item-radius); border: 1px solid var(--border); background: transparent; padding: 0.375rem 0.625rem; color: var(--foreground); outline: none; }
  :global(.setup-secondary-button), :global(.setup-primary-button) { display: inline-flex; min-height: 2.25rem; align-items: center; justify-content: center; border-radius: 0.375rem; padding: 0.375rem 0.75rem; font-size: calc(0.8rem * var(--type-scale)); font-weight: 600; }
  :global(.setup-secondary-button) { border: 1px solid var(--border); background: var(--background); }
  :global(.setup-primary-button) { background: var(--primary); color: var(--primary-foreground); }
  :global(.setup-primary-button:disabled), :global(.setup-secondary-button:disabled) { cursor: not-allowed; opacity: 0.5; }
</style>
