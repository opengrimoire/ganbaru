<script lang="ts">
  import { onMount } from "svelte";
  import { chatErrorMessage } from "$lib/chat/error-presentation";
  import { cn } from "$lib/utils";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import { getChat } from "$lib/stores/chat.svelte";
  import type { AgentsSettingsSubsection, ChatProviderSetupTarget } from "$lib/settings/types";
  import AgentFilesSettings from "$lib/components/settings/agents/AgentFilesSettings.svelte";
  import ProvidersSettings from "$lib/components/settings/agents/ProvidersSettings.svelte";
  import Loading from "$lib/components/ui/Loading.svelte";
  import Notice from "$lib/components/ui/Notice.svelte";

  let {
    initialSubsection,
    onOpenProviderSetup = () => {},
    onSubsectionChange = () => {},
    onRequestNavigation = (navigate) => navigate(),
  }: {
    initialSubsection?: AgentsSettingsSubsection;
    onOpenProviderSetup?: (target: ChatProviderSetupTarget) => void;
    onSubsectionChange?: (subsection: AgentsSettingsSubsection) => void;
    onRequestNavigation?: (navigate: () => void) => void;
  } = $props();

  const { t } = getLocalization();
  const chat = getChat();
  let activeTab = $state<AgentsSettingsSubsection>("providers");
  let initializing = $state(!chat.loaded);
  let loadError = $state<string | null>(null);
  const tabs: ReadonlyArray<{
    id: AgentsSettingsSubsection;
    label: () => string;
  }> = [
    { id: "providers", label: () => t("settings.agents.providers.heading") },
    { id: "files", label: () => t("settings.agents.files.heading") },
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
      loadError = chatErrorMessage(error, t("settings.agents.loadFailed"));
    } finally {
      initializing = false;
    }
  }

  /** Retries loading agent settings and rejects when the load fails again. */
  async function retryInitialize(): Promise<void> {
    await initialize();
    if (loadError) throw new Error(loadError);
  }

  function selectTab(tab: AgentsSettingsSubsection): void {
    if (tab === activeTab) return;
    onRequestNavigation(() => {
      activeTab = tab;
      onSubsectionChange(tab);
    });
  }
</script>

<div class="grid h-full min-h-0 grid-rows-[auto_minmax(0,1fr)] gap-6">
  <div
    class="grid grid-cols-2 gap-1 rounded-md border border-border bg-card p-1 dark:bg-transparent"
    role="tablist"
    aria-label={t("settings.agents.tabLabel")}
  >
    {#each tabs as tab (tab.id)}
      {@const active = activeTab === tab.id}
      <button
        type="button"
        role="tab"
        aria-selected={active}
        class={cn(
          "flex min-h-8 min-w-0 items-center justify-center rounded-sm px-2 text-center text-[0.8rem] font-medium text-muted-foreground",
          active && "bg-background text-foreground dark:bg-foreground/5",
        )}
        onclick={() => selectTab(tab.id)}
      >
        <span class="truncate">{tab.label()}</span>
      </button>
    {/each}
  </div>

  {#if initializing}
    <Loading layout="fill" class="min-h-40" />
  {:else if loadError}
    <div class="min-h-40 text-sm"><Notice layout="fill" message={loadError} onRetry={retryInitialize} /></div>
  {:else if activeTab === "providers"}
    <ProvidersSettings {onOpenProviderSetup} />
  {:else if activeTab === "files"}
    <AgentFilesSettings {onOpenProviderSetup} />
  {/if}
</div>
