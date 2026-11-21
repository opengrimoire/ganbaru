<script lang="ts">
  import Check from "@lucide/svelte/icons/check";
  import Copy from "@lucide/svelte/icons/copy";
  import Download from "@lucide/svelte/icons/download";
  import ExternalLink from "@lucide/svelte/icons/external-link";
  import { onDestroy } from "svelte";
  import { openUrl } from "@tauri-apps/plugin-opener";
  import type { ProviderInstanceId } from "$lib/chat/contracts";
  import ChatModelAvatar from "$lib/components/chat/identity/ChatModelAvatar.svelte";
  import { SETTINGS_PRIMARY_BUTTON_CLASS, SETTINGS_SECONDARY_BUTTON_CLASS, SETTINGS_ICON_BUTTON_CLASS } from "$lib/components/settings/action-buttons";
  import Notice from "$lib/components/ui/Notice.svelte";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import { getChat } from "$lib/stores/chat.svelte";
  import { getProviderUpdates } from "$lib/stores/chat/provider-updates.svelte";
  import { getUpdateManager } from "$lib/stores/updates.svelte";
  import { cn } from "$lib/utils";
  import { writeTextToClipboard } from "$lib/utils/clipboard";
  import { scrollEdgeFadeAction } from "$lib/utils/scroll-edge-fade";
  import { appUpdatePending, cliUpdateEntries, offersUpdateAll, runnableCliUpdates } from "./updates-control";

  /**
   * Title bar indicator that appears while Ganbaru or a configured agent CLI has an update, and
   * opens a panel with the release notes and the actions to apply each one.
   */
  let {
    onOpened,
  }: {
    /** Called when the panel opens, so other title bar menus can close. */
    onOpened?: () => void;
  } = $props();

  const COPY_FEEDBACK_MS = 1_600;

  const { t } = getLocalization();
  const updates = getUpdateManager();
  const providerUpdates = getProviderUpdates();
  const chat = getChat();

  let open = $state(false);
  /** The copy button that just succeeded, keyed by provider instance or `app`. */
  let copiedKey = $state<string | null>(null);
  /** The output of each CLI update that failed, or `null` when the command could not start. */
  let failures = $state<Partial<Record<ProviderInstanceId, string | null>>>({});
  let updatingAll = $state(false);
  let copyTimer: ReturnType<typeof setTimeout> | undefined;

  const showApp = $derived(appUpdatePending(updates.status));
  const cliEntries = $derived(
    cliUpdateEntries(providerUpdates.updates, chat.settings?.providerInstances ?? [], Date.now()),
  );
  const visible = $derived(showApp || cliEntries.length > 0);
  const appSelfUpdate = $derived(updates.status === "available" && updates.primaryAction === "self-updater");
  const runnableCli = $derived(runnableCliUpdates(cliEntries));
  const showUpdateAll = $derived(offersUpdateAll(appSelfUpdate, runnableCli.length));

  $effect(() => {
    if (!visible) open = false;
  });

  function toggle(): void {
    open = !open;
    if (open) onOpened?.();
  }

  async function copy(key: string, text: string): Promise<void> {
    try {
      await writeTextToClipboard(text);
      copiedKey = key;
      if (copyTimer) clearTimeout(copyTimer);
      copyTimer = setTimeout(() => {
        copiedKey = null;
      }, COPY_FEEDBACK_MS);
    } catch (error: unknown) {
      console.error("Failed to copy the update command:", error);
    }
  }

  async function openReleasePage(): Promise<void> {
    const url = updates.releasePageUrl;
    if (!url) return;
    try {
      await openUrl(url);
    } catch (error: unknown) {
      console.error("Failed to open release page:", error);
    }
  }

  function setFailure(instanceId: ProviderInstanceId, output: string | null | undefined): void {
    const next = { ...failures };
    if (output === undefined) delete next[instanceId];
    else next[instanceId] = output;
    failures = next;
  }

  /**
   * Runs one CLI update and probes the provider again so its new version shows.
   *
   * @returns Whether the update finished.
   */
  async function runCliUpdate(instanceId: ProviderInstanceId): Promise<boolean> {
    setFailure(instanceId, undefined);
    try {
      const result = await providerUpdates.run(instanceId);
      if (!result.successful) {
        setFailure(instanceId, result.output);
        return false;
      }
      await chat.probeProvider(instanceId);
      return true;
    } catch (error: unknown) {
      console.error("Failed to update the provider CLI:", error);
      setFailure(instanceId, null);
      return false;
    }
  }

  /**
   * Runs every CLI update one after another, then installs the app update last, since it restarts the app. The app
   * waits when a CLI update fails, so its notice stays visible.
   */
  async function updateEverything(): Promise<void> {
    updatingAll = true;
    try {
      let allFinished = true;
      for (const instanceId of runnableCli) {
        if (!(await runCliUpdate(instanceId))) allFinished = false;
      }
      if (allFinished && appSelfUpdate) await updates.installUpdate();
    } finally {
      updatingAll = false;
    }
  }

  onDestroy(() => {
    if (copyTimer) clearTimeout(copyTimer);
  });
</script>

{#if visible}
  <div class="relative shrink-0">
    <button
      type="button"
      class={cn(
        "flex size-7 items-center justify-center rounded-lg transition-colors",
        open ? "bg-sidebar-accent" : "hover:bg-sidebar-accent",
      )}
      title={t("updates.panel.trigger")}
      aria-label={t("updates.panel.trigger")}
      aria-haspopup="dialog"
      aria-expanded={open}
      onclick={toggle}
    >
      <!-- Same ring geometry as the Focus ring beside it, with an upgrade arrow inside. -->
      <svg viewBox="0 0 20 20" width="14.5" height="14.5" aria-hidden="true" class="fill-none stroke-foreground/68 dark:stroke-white/76">
        <circle cx="10" cy="10" r="8" stroke-width="2.15" />
        <path d="M10 13.25V6.75M7.25 9.5 10 6.75l2.75 2.75" stroke-width="1.9" stroke-linecap="round" stroke-linejoin="round" />
      </svg>
    </button>

    {#if open}
      <!-- svelte-ignore a11y_no_static_element_interactions -->
      <div
        class="fixed inset-0 z-40"
        onclick={() => { open = false; }}
        onkeydown={(event) => { if (event.key === "Escape") open = false; }}
      ></div>
      <div
        role="dialog"
        aria-label={t("updates.panel.trigger")}
        class="surface-floating absolute right-0 top-9 z-50 flex max-h-[calc(100vh-3rem)] w-floating-lg max-w-[calc(100vw-1rem)] flex-col overflow-hidden"
      >
        <div use:scrollEdgeFadeAction class="flex min-h-0 flex-1 flex-col gap-4 overflow-y-auto p-3 text-panel">
          {#if showApp}
            <section class="flex flex-col gap-2">
              <h3 class="text-panel-detail font-semibold text-muted-foreground">{t("updates.panel.appHeading")}</h3>
              <div class="flex items-center gap-2.5">
                <span class="min-w-0 flex-1 font-medium text-foreground">
                  {updates.currentVersion && updates.latestVersion
                    ? t("updates.panel.versionChange", updates.currentVersion, updates.latestVersion)
                    : t("updates.versionAvailable", updates.latestVersion ?? "unknown")}
                </span>
                {#if updates.status === "available"}
                  {#if updates.primaryAction === "self-updater"}
                    <button type="button" class={SETTINGS_PRIMARY_BUTTON_CLASS} onclick={() => void updates.installUpdate()}>
                      <Download size={14} strokeWidth={1.9} class="shrink-0" aria-hidden="true" />
                      <span>{t("updates.updateAndRestart")}</span>
                    </button>
                  {:else if updates.primaryAction === "copy-command" && updates.copyCommand}
                    {@const command = updates.copyCommand}
                    <button type="button" class={SETTINGS_PRIMARY_BUTTON_CLASS} onclick={() => void copy("app", command)}>
                      <Copy size={14} strokeWidth={1.9} class="shrink-0" aria-hidden="true" />
                      <span>{copiedKey === "app" ? t("updates.commandCopied") : t("updates.copyCommand")}</span>
                    </button>
                  {/if}
                {/if}
              </div>
              {#if updates.status === "downloading"}
                <div class="text-panel-detail text-muted-foreground">{updates.statusCopy}</div>
                <div class="h-1.5 overflow-hidden rounded-full bg-muted">
                  <div class="h-full rounded-full bg-primary transition-[width]" style={`width: ${updates.progressPercent ?? 18}%;`}></div>
                </div>
              {:else if updates.status === "installed"}
                <div class="text-panel-detail text-muted-foreground">{t("updates.installedRestarting")}</div>
              {/if}
              {#if updates.releaseNotes}
                <div class="max-h-48 overflow-y-auto whitespace-pre-line wrap-break-word rounded-md border border-border bg-background/60 px-2.5 py-2 text-panel-detail text-muted-foreground">
                  {updates.releaseNotes}
                </div>
              {/if}
              {#if updates.releasePageUrl}
                <button type="button" class={cn(SETTINGS_SECONDARY_BUTTON_CLASS, "self-start")} onclick={() => void openReleasePage()}>
                  <ExternalLink size={14} strokeWidth={1.9} class="shrink-0" aria-hidden="true" />
                  <span>{t("updates.releaseNotes")}</span>
                </button>
              {/if}
            </section>
          {/if}

          {#if showApp && cliEntries.length > 0}
            <div class="h-px shrink-0 bg-border/50" aria-hidden="true"></div>
          {/if}

          {#if cliEntries.length > 0}
            <section class="flex flex-col gap-2">
              <h3 class="text-panel-detail font-semibold text-muted-foreground">{t("updates.panel.cliHeading")}</h3>
              {#each cliEntries as { provider, update } (update.instanceId)}
                {@const label = provider.configuration.label}
                <div class="flex flex-col gap-1.5">
                  <div class="flex items-center gap-2.5">
                    <ChatModelAvatar familyId={provider.configuration.familyId} {label} size={24} />
                    <span class="min-w-0 flex-1">
                      <span class="block truncate font-medium text-foreground">{label}</span>
                      <span class="block text-panel-detail text-muted-foreground">{t("updates.panel.versionChange", update.currentVersion, update.latestVersion)}</span>
                    </span>
                    {#if update.runsInApp}
                      <button
                        type="button"
                        class={SETTINGS_SECONDARY_BUTTON_CLASS}
                        disabled={providerUpdates.running !== null || updatingAll}
                        onclick={() => void runCliUpdate(update.instanceId)}
                      >{providerUpdates.running === update.instanceId ? t("updates.panel.updating") : t("updates.panel.update")}</button>
                      <button
                        type="button"
                        class={SETTINGS_ICON_BUTTON_CLASS}
                        title={t("updates.panel.copyCommand")}
                        aria-label={t("updates.panel.copyCommand")}
                        onclick={() => void copy(update.instanceId, update.updateCommand)}
                      >{#if copiedKey === update.instanceId}<Check size={14} aria-hidden="true" />{:else}<Copy size={14} aria-hidden="true" />{/if}</button>
                    {/if}
                  </div>
                  {#if !update.runsInApp}
                    <p class="text-panel-detail text-muted-foreground">{t("updates.panel.managed")}</p>
                  {/if}
                  {#if update.instanceId in failures}
                    <Notice
                      class="text-panel-detail"
                      tone="warning"
                      message={t("updates.panel.updateFailed")}
                      copy={failures[update.instanceId] ?? update.updateCommand}
                      onDismiss={() => setFailure(update.instanceId, undefined)}
                    />
                  {/if}
                </div>
              {/each}
            </section>
          {/if}

          {#if showUpdateAll}
            <button
              type="button"
              class={cn(SETTINGS_PRIMARY_BUTTON_CLASS, "w-full justify-center")}
              disabled={providerUpdates.running !== null || updatingAll}
              onclick={() => void updateEverything()}
            >
              <Download size={14} strokeWidth={1.9} class="shrink-0" aria-hidden="true" />
              <span>{updatingAll ? t("updates.panel.updating") : t("updates.panel.updateAll")}</span>
            </button>
          {/if}
        </div>
      </div>
    {/if}
  </div>
{/if}
