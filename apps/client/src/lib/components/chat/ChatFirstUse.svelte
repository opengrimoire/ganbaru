<script lang="ts">
  import Folder from "@lucide/svelte/icons/folder";
  import FolderSearch from "@lucide/svelte/icons/folder-search";
  import GitBranch from "@lucide/svelte/icons/git-branch";
  import Laptop from "@lucide/svelte/icons/laptop";
  import MessageSquare from "@lucide/svelte/icons/message-square";
  import Settings from "@lucide/svelte/icons/settings";
  import Unplug from "@lucide/svelte/icons/unplug";
  import type { ProviderInstanceId, ProviderProbeResult } from "$lib/chat/contracts";
  import { resolveChatFirstUseState } from "$lib/chat/shell-model";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import { useChat } from "$lib/stores/chat.svelte";
  import { getNavigation } from "$lib/stores/navigation.svelte";
  import { useProjects } from "$lib/stores/projects.svelte";
  import { getSettingsLauncher } from "$lib/stores/settings-launcher.svelte";
  import ChatComposer from "$lib/components/chat/composer/ChatComposer.svelte";
  import ChatTimeline from "$lib/components/chat/timeline/ChatTimeline.svelte";
  import Notice from "$lib/components/ui/Notice.svelte";

  const { t } = getLocalization();
  const chat = useChat();
  const navigation = getNavigation();
  const projects = useProjects();
  const settings = getSettingsLauncher();
  let operationError = $state<string | null>(null);
  const firstUse = $derived(resolveChatFirstUseState({ providers: chat.settings?.providerInstances ?? [], workingFolders: chat.workingFolders, selectedProjectId: projects.selectedProjectId, selectedProjectArchived: projects.selectedProject?.status === "archived", selectedWorkingFolderId: chat.selectedWorkingFolderId, selectedThreadId: chat.selectedThreadId, threads: [...chat.activeThreads, ...chat.archivedThreads] }));
  const workspace = $derived(chat.selectedWorkingFolder);

  function run(action: () => Promise<unknown>): void {
    operationError = null;
    void action().catch((error: unknown) => {
      operationError = error instanceof Error ? error.message : String(error);
    });
  }

  /** Probes the provider again and rejects unless it reports healthy, so the notice can show the retry outcome. */
  async function retryProvider(instanceId: ProviderInstanceId): Promise<void> {
    operationError = null;
    let result: ProviderProbeResult;
    try {
      result = await chat.probeProvider(instanceId);
    } catch (error: unknown) {
      operationError = error instanceof Error ? error.message : String(error);
      throw error;
    }
    if (result.state !== "healthy") throw new Error(`Chat provider probe reported ${result.state}`);
  }

</script>

<div class="first-use-shell" class:new-thread={firstUse.kind === "no_thread"} class:pending-send={firstUse.kind === "no_thread" && chat.pendingUserMessage !== null}>
  <section class="first-use-content" class:new-thread={firstUse.kind === "no_thread"} class:pending-send={firstUse.kind === "no_thread" && chat.pendingUserMessage !== null}>
    {#if operationError}<Notice class="mb-3 text-sm" message={operationError} />{/if}
    {#if firstUse.kind !== "no_thread" && firstUse.kind !== "provider_unavailable"}<div class="mb-4 flex size-12 items-center justify-center rounded-2xl border border-border bg-card"><MessageSquare size={22} /></div>{/if}
    {#if firstUse.kind === "no_provider"}
      <h2 class="text-lg font-semibold">{t("chat.firstUse.noProviderTitle")}</h2><p class="mt-2 max-w-lg text-sm text-muted-foreground">{t("chat.firstUse.noProviderDescription")}</p><div class="mt-5 flex flex-wrap justify-center gap-2"><button type="button" class="chat-primary-button" onclick={() => settings.open("chat", { chatSubsection: "providers" })}><Settings size={15} />{t("chat.firstUse.setUpProvider")}</button></div><div class="mt-5 flex flex-wrap justify-center gap-2 text-xs text-muted-foreground"><span>Codex</span><span>Claude</span><span>Cursor</span><span>OpenCode</span></div>
    {:else if firstUse.kind === "no_project"}
      <h2 class="text-lg font-semibold">{t("chat.firstUse.noProjectTitle")}</h2><p class="mt-2 max-w-lg text-sm text-muted-foreground">{t("chat.firstUse.noProjectDescription")}</p><button type="button" class="chat-primary-button mt-5" onclick={() => navigation.navigate("projects")}>{t("chat.firstUse.openProjects")}</button>
    {:else if firstUse.kind === "archived_project"}
      <h2 class="text-lg font-semibold">{t("chat.firstUse.archivedProjectTitle")}</h2><p class="mt-2 max-w-lg text-sm text-muted-foreground">{t("chat.firstUse.archivedProjectDescription")}</p><button type="button" class="chat-primary-button mt-5" onclick={() => navigation.navigate("projects")}>{t("chat.firstUse.openProjects")}</button>
    {:else if firstUse.kind === "missing_binding"}
      <h2 class="text-lg font-semibold">{t("chat.firstUse.missingBindingTitle")}</h2><p class="mt-2 max-w-lg text-sm text-muted-foreground">{t("chat.firstUse.missingBindingDescription", firstUse.workingFolder.workingFolder.displayName)}</p>{#if firstUse.workingFolder.workingFolder.repositoryIdentity}<code class="mt-3 max-w-full truncate text-xs text-muted-foreground">{firstUse.workingFolder.workingFolder.repositoryIdentity}</code>{/if}<button type="button" class="chat-primary-button mt-5" onclick={() => run(() => firstUse.workingFolder.workingFolder.kind === "managed" ? chat.recreateManagedWorkingFolder(firstUse.workingFolder.workingFolder.id) : chat.rebindWorkingFolder(firstUse.workingFolder.workingFolder.id, t("chat.firstUse.chooseWorkingFolder")))}><FolderSearch size={15} />{firstUse.workingFolder.workingFolder.kind === "managed" ? t("chat.firstUse.recreateFolder") : t("chat.firstUse.locateFolder")}</button>
    {:else if firstUse.kind === "provider_unavailable"}
      {@const unavailableProvider = firstUse.provider}
      <div class="w-full text-sm"><Notice layout="fill" role="status" tone="warning" icon={Unplug} title={t("chat.firstUse.unavailableTitle")} message={unavailableProvider?.lastProbe?.detail ?? t("chat.status.providerUnavailable")} retryLabel={t("chat.firstUse.retryProvider")} onRetry={unavailableProvider ? () => retryProvider(unavailableProvider.configuration.instanceId) : undefined} copy={unavailableProvider?.lastProbe?.detail ?? false}>{#snippet actions()}<button type="button" class="notice-action-text" onclick={() => settings.open("chat", { chatSubsection: "providers" })}>{t("chat.firstUse.openSettings")}</button>{/snippet}</Notice></div>
    {:else if firstUse.kind === "archived_thread"}
      <h2 class="text-lg font-semibold">{t("chat.firstUse.archivedTitle")}</h2><p class="mt-2 max-w-lg text-sm text-muted-foreground">{t("chat.firstUse.archivedDescription")}</p><button type="button" class="chat-primary-button mt-5" onclick={() => run(() => chat.restoreThread(firstUse.thread))}>{t("chat.restore")}</button>
    {:else if firstUse.kind === "no_thread"}
      <div class="hero-composer-shell" class:pending-send={chat.pendingUserMessage !== null}>
        {#if chat.pendingUserMessage}
          <ChatTimeline />
        {:else if workspace}
          <div class="working-folder-context" aria-label={t("chat.hero.workingFolder")}>
            <button type="button" title={t("chat.openFolder")} onclick={() => run(() => chat.openWorkingFolder(workspace.workingFolder.id))}>
              <Folder size={15} /><span>{workspace.workingFolder.displayName}</span>
            </button>
            <span><Laptop size={15} />{t("chat.header.local")}</span>
            {#if workspace.currentBranch}<span title={t("chat.header.branch", workspace.currentBranch)}><GitBranch size={15} /><span>{workspace.currentBranch}</span></span>{/if}
          </div>
        {/if}
        <div class:pending-composer-dock={chat.pendingUserMessage !== null}><ChatComposer hero={chat.pendingUserMessage === null} /></div>
      </div>
    {:else}
      <h2 class="text-lg font-semibold">{firstUse.thread.title}</h2>
      <p class="mt-2 max-w-lg text-sm text-muted-foreground">{t("chat.hero.conversationPending")}</p>
    {/if}
  </section>
</div>

<style>
  .first-use-shell { display: flex; min-height: 0; flex: 1; align-items: center; justify-content: center; overflow-y: auto; padding: 1rem; }
  .first-use-content { display: flex; width: 100%; max-width: 42rem; flex-direction: column; align-items: center; text-align: center; }
  .first-use-shell.new-thread { align-items: stretch; }
  .first-use-content.new-thread { max-width: none; justify-content: flex-end; padding-bottom: clamp(0.25rem, 2vh, 1.5rem); }
  .hero-composer-shell { width:min(100%,var(--chat-conversation-max-width,60rem)); text-align: left; }
  .first-use-shell.pending-send { padding: 0; }
  .first-use-content.pending-send { height: 100%; align-items: stretch; padding-bottom: 0; }
  .hero-composer-shell.pending-send { position: relative; display: flex; width: 100%; max-width: none; min-height: 0; flex: 1; flex-direction: column; overflow: hidden; }
  .pending-composer-dock { pointer-events: none; position: absolute; inset-inline: 0; bottom: 0; z-index: 2; padding: 0.5rem 0.75rem 0.75rem; }
  .pending-composer-dock::before { position: absolute; inset: -1.5rem 0 -2rem; z-index: -1; background: linear-gradient(to bottom, transparent, color-mix(in srgb, var(--cal-bg) 72%, transparent) 35%, var(--cal-bg) 74%); content: ""; -webkit-mask-image: linear-gradient(to bottom, transparent, black 35%); mask-image: linear-gradient(to bottom, transparent, black 35%); }
  .pending-composer-dock :global(.chat-composer) { pointer-events: auto; }
  .working-folder-context { display: flex; min-width: 0; min-height: 3.15rem; align-items: center; gap: 1.2rem; margin-inline: 1.35rem; border-radius: 1.2rem 1.2rem 0 0; background: color-mix(in srgb, var(--muted) 72%, transparent); padding: 0.45rem 1.1rem 0.7rem; color: var(--foreground); font-size: calc(0.8rem * var(--type-scale)); }
  .working-folder-context > button, .working-folder-context > span { display: flex; min-width: 0; align-items: center; gap: 0.45rem; }
  .working-folder-context > button { max-width: 45%; border-radius: 0.4rem; }
  .working-folder-context > button:hover { color: var(--primary); }
  .working-folder-context span span, .working-folder-context button span { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .working-folder-context :global(svg) { flex: 0 0 auto; }
  .hero-composer-shell :global(.chat-composer.hero) { margin-top: -0.45rem; }
  :global(.chat-primary-button), :global(.chat-secondary-button) { display: inline-flex; min-height: 2.25rem; align-items: center; justify-content: center; gap: 0.4rem; border-radius: 0.375rem; padding: 0.4rem 0.8rem; font-size: calc(0.8rem * var(--type-scale)); font-weight: 600; }
  :global(.chat-primary-button) { background: var(--primary); color: var(--primary-foreground); }
  :global(.chat-secondary-button) { border: 1px solid var(--border); background: var(--background); color: var(--foreground); }
  @container chat-shell (max-width: 560px) { .working-folder-context { gap: 0.75rem; margin-inline: 0.65rem; } .working-folder-context > button { max-width: 55%; } .working-folder-context > span:nth-child(2) { display: none; } }
  @media (max-height: 520px) { .first-use-content.new-thread { padding-bottom: 0; } }
</style>
