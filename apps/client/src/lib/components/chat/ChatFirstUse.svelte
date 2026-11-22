<script lang="ts">
  import FolderSearch from "@lucide/svelte/icons/folder-search";
  import MessageSquare from "@lucide/svelte/icons/message-square";
  import Settings from "@lucide/svelte/icons/settings";
  import Unplug from "@lucide/svelte/icons/unplug";
  import type { ProviderInstanceId, ProviderProbeResult } from "$lib/chat/contracts";
  import { resolveChatFirstUseState, type ChatFirstUseState } from "$lib/chat/shell-model";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import { chatErrorMessage, providerProbeMessage } from "$lib/chat/error-presentation";
  import { LocalizedError } from "$lib/i18n/user-errors";
  import { useChat } from "$lib/stores/chat.svelte";
  import { getNavigation } from "$lib/stores/navigation.svelte";
  import { useProjects } from "$lib/stores/projects.svelte";
  import { getSettingsLauncher } from "$lib/stores/settings-launcher.svelte";
  import Notice from "$lib/components/ui/Notice.svelte";

  const { t } = getLocalization();
  const chat = useChat();
  const navigation = getNavigation();
  const projects = useProjects();
  const settings = getSettingsLauncher();
  /** The last failed action and the first-use target it acted on; shown only while that target is still shown. */
  let operationError = $state.raw<{ target: string; message: string } | null>(null);
  const firstUse = $derived(resolveChatFirstUseState({ providers: chat.settings?.providerInstances ?? [], workingFolders: chat.workingFolders, selectedProjectId: projects.selectedProjectId, selectedProjectArchived: projects.selectedProject?.status === "archived", selectedWorkingFolderId: chat.selectedWorkingFolderId, selectedThreadId: chat.selectedThreadId, threads: [...chat.activeThreads, ...chat.archivedThreads] }));
  const firstUseTarget = $derived(targetKey(firstUse));
  const visibleOperationError = $derived(operationError?.target === firstUseTarget ? operationError.message : null);

  /**
   * Names the item a first-use action acts on, so a failure stays with that item.
   *
   * @param state The resolved first-use state.
   * @returns A key that changes whenever the shown item changes.
   */
  function targetKey(state: ChatFirstUseState): string {
    switch (state.kind) {
      case "missing_binding":
      case "no_thread":
        return `${state.kind}:${state.workingFolder.workingFolder.id}`;
      case "provider_unavailable":
        return `${state.kind}:${state.providerInstanceId}`;
      case "archived_thread":
      case "conversation":
        return `${state.kind}:${state.thread.id}`;
      default:
        return state.kind;
    }
  }

  /**
   * Runs a first-use action and records its failure against the item shown when it started.
   *
   * @param action The action to run.
   */
  function run(action: () => Promise<unknown>): void {
    const target = firstUseTarget;
    operationError = null;
    void action().catch((error: unknown) => {
      operationError = { target, message: chatErrorMessage(error) };
    });
  }

  /** Probes the provider again and rejects unless it reports healthy, so the notice can show the retry outcome. */
  async function retryProvider(instanceId: ProviderInstanceId): Promise<void> {
    const target = firstUseTarget;
    operationError = null;
    let result: ProviderProbeResult;
    try {
      result = await chat.probeProvider(instanceId);
    } catch (error: unknown) {
      operationError = { target, message: chatErrorMessage(error) };
      throw error;
    }
    if (result.state !== "healthy") throw new LocalizedError(providerProbeMessage(result.state));
  }

</script>

<div data-mobile-scroll-fade class="first-use-shell">
  <section class="first-use-content">
    {#if visibleOperationError}<Notice class="mb-3 text-sm" message={visibleOperationError} />{/if}
    {#if firstUse.kind !== "no_thread" && firstUse.kind !== "provider_unavailable"}<div class="mb-4 flex size-12 items-center justify-center rounded-2xl border border-border bg-card"><MessageSquare size={22} /></div>{/if}
    {#if firstUse.kind === "no_provider"}
      <h2 class="text-lg font-semibold">{t("chat.firstUse.noProviderTitle")}</h2><p class="mt-2 max-w-lg text-sm text-muted-foreground">{t("chat.firstUse.noProviderDescription")}</p><div class="mt-5 flex flex-wrap justify-center gap-2"><button type="button" class="chat-primary-button" onclick={() => settings.open("agents", { agentsSubsection: "providers" })}><Settings size={15} />{t("chat.firstUse.setUpProvider")}</button></div><div class="mt-5 flex flex-wrap justify-center gap-2 text-xs text-muted-foreground"><span>Codex</span><span>Claude</span><span>Cursor</span><span>OpenCode</span></div>
    {:else if firstUse.kind === "no_project"}
      <h2 class="text-lg font-semibold">{t("chat.firstUse.noProjectTitle")}</h2><p class="mt-2 max-w-lg text-sm text-muted-foreground">{t("chat.firstUse.noProjectDescription")}</p><button type="button" class="chat-primary-button mt-5" onclick={() => navigation.navigate("projects")}>{t("chat.firstUse.openProjects")}</button>
    {:else if firstUse.kind === "archived_project"}
      <h2 class="text-lg font-semibold">{t("chat.firstUse.archivedProjectTitle")}</h2><p class="mt-2 max-w-lg text-sm text-muted-foreground">{t("chat.firstUse.archivedProjectDescription")}</p><button type="button" class="chat-primary-button mt-5" onclick={() => navigation.navigate("projects")}>{t("chat.firstUse.openProjects")}</button>
    {:else if firstUse.kind === "missing_binding"}
      <h2 class="text-lg font-semibold">{t("chat.firstUse.missingBindingTitle")}</h2><p class="mt-2 max-w-lg text-sm text-muted-foreground">{t("chat.firstUse.missingBindingDescription", firstUse.workingFolder.workingFolder.displayName)}</p>{#if firstUse.workingFolder.workingFolder.repositoryIdentity}<code class="mt-3 max-w-full truncate text-xs text-muted-foreground">{firstUse.workingFolder.workingFolder.repositoryIdentity}</code>{/if}<button type="button" class="chat-primary-button mt-5" onclick={() => run(() => firstUse.workingFolder.workingFolder.kind === "managed" ? chat.recreateManagedWorkingFolder(firstUse.workingFolder.workingFolder.id) : chat.rebindWorkingFolder(firstUse.workingFolder.workingFolder.id, t("chat.firstUse.chooseWorkingFolder")))}><FolderSearch size={15} />{firstUse.workingFolder.workingFolder.kind === "managed" ? t("chat.firstUse.recreateFolder") : t("chat.firstUse.locateFolder")}</button>
    {:else if firstUse.kind === "provider_unavailable"}
      {@const unavailableProvider = firstUse.provider}
      <div class="w-full text-sm"><Notice layout="fill" role="status" tone="warning" icon={Unplug} title={t("chat.firstUse.unavailableTitle")} message={providerProbeMessage(unavailableProvider?.lastProbe?.state)} retryLabel={t("chat.firstUse.retryProvider")} onRetry={unavailableProvider ? () => retryProvider(unavailableProvider.configuration.instanceId) : undefined} copy={unavailableProvider?.lastProbe?.detail ?? false}>{#snippet actions()}<button type="button" class="notice-action-text" onclick={() => settings.open("agents", { agentsSubsection: "providers" })}>{t("chat.firstUse.openSettings")}</button>{/snippet}</Notice></div>
    {:else if firstUse.kind === "archived_thread"}
      <h2 class="text-lg font-semibold">{t("chat.firstUse.archivedTitle")}</h2><p class="mt-2 max-w-lg text-sm text-muted-foreground">{t("chat.firstUse.archivedDescription")}</p><button type="button" class="chat-primary-button mt-5" onclick={() => run(() => chat.restoreThread(firstUse.thread))}>{t("chat.restore")}</button>
    {:else if firstUse.kind === "conversation"}
      <h2 class="text-lg font-semibold">{firstUse.thread.title}</h2>
      <p class="mt-2 max-w-lg text-sm text-muted-foreground">{t("chat.hero.conversationPending")}</p>
    {/if}
  </section>
</div>

<style>
  .first-use-shell { display: flex; min-height: 0; flex: 1; align-items: center; justify-content: center; overflow-y: auto; padding: 1rem; }
  .first-use-content { display: flex; width: 100%; max-width: 42rem; flex-direction: column; align-items: center; text-align: center; }
  :global(.chat-primary-button), :global(.chat-secondary-button) { display: inline-flex; min-height: 2.25rem; align-items: center; justify-content: center; gap: 0.4rem; border-radius: 0.375rem; padding: 0.4rem 0.8rem; font-size: calc(0.8rem * var(--type-scale)); font-weight: 600; }
  :global(.chat-primary-button) { background: var(--primary); color: var(--primary-foreground); }
  :global(.chat-secondary-button) { border: 1px solid var(--border); background: var(--background); color: var(--foreground); }
</style>
