<script lang="ts">
  import Copy from "@lucide/svelte/icons/copy";
  import Download from "@lucide/svelte/icons/download";
  import ExternalLink from "@lucide/svelte/icons/external-link";
  import X from "@lucide/svelte/icons/x";
  import { onDestroy } from "svelte";
  import { openUrl } from "@tauri-apps/plugin-opener";
  import { getSettingsLauncher } from "$lib/stores/settings-launcher.svelte";
  import { getUpdateManager } from "$lib/stores/updates.svelte";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import { cn } from "$lib/utils";

  const settingsLauncher = getSettingsLauncher();
  const updates = getUpdateManager();
  const { t } = getLocalization();
  let commandCopied = $state(false);
  let commandCopyTimer: ReturnType<typeof setTimeout> | undefined;

  const message = $derived(
    updates.status === "installed"
      ? t("updates.installedRestarting")
      : updates.status === "downloading"
        ? updates.statusCopy
        : t("updates.promptAvailable", updates.latestVersion ?? "unknown"),
  );

  async function openReleasePage(): Promise<void> {
    const url = updates.releasePageUrl;
    if (!url) return;
    updates.dismissPrompt();
    try {
      await openUrl(url);
    } catch (error: unknown) {
      console.error("Failed to open release page:", error);
      settingsLauncher.open("updates");
    }
  }

  async function copyUpdateCommand(): Promise<void> {
    const command = updates.copyCommand;
    if (!command) return;
    try {
      await navigator.clipboard.writeText(command);
      commandCopied = true;
      if (commandCopyTimer) clearTimeout(commandCopyTimer);
      commandCopyTimer = setTimeout(() => {
        commandCopied = false;
      }, 1600);
    } catch (error: unknown) {
      console.error("Failed to copy update command:", error);
      settingsLauncher.open("updates");
    }
  }

  onDestroy(() => {
    if (commandCopyTimer) clearTimeout(commandCopyTimer);
  });
</script>

{#if updates.showPrompt}
  <div
    role="status"
    aria-live="polite"
    class="fixed right-3 bottom-3 z-80 flex w-[min(28rem,calc(100vw-1.5rem))] gap-3 surface-floating px-3 py-3 max-[420px]:right-2 max-[420px]:bottom-2 max-[420px]:w-[calc(100vw-1rem)]"
  >
    <div class="min-w-0 flex-1">
      <p class="wrap-break-word text-[0.866667rem] font-medium leading-5">
        {message}
      </p>

      {#if updates.status === "downloading"}
        <div class="mt-2 h-2 overflow-hidden rounded-full bg-muted">
          <div
            class="h-full rounded-full bg-primary transition-[width]"
            style={`width: ${updates.progressPercent ?? 18}%;`}
          ></div>
        </div>
        <p class="mt-1 text-[0.733333rem] text-muted-foreground">
          {#if updates.contentLength}
            {t(
              "updates.downloadedOfTotal",
              updates.formatBytes(updates.downloadedBytes),
              updates.formatBytes(updates.contentLength),
            )}
          {:else}
            {t("updates.downloaded", updates.formatBytes(updates.downloadedBytes))}
          {/if}
        </p>
      {/if}

      {#if updates.status === "available"}
        <div class="mt-2 flex flex-wrap gap-1.5">
          {#if updates.primaryAction === "self-updater"}
            <button
              type="button"
              class="inline-flex h-8 items-center justify-center gap-1.5 rounded-md bg-primary px-2.5 text-[0.8rem] font-medium text-primary-foreground transition-colors hover:bg-primary/90 focus:outline-none focus:ring-1 focus:ring-ring"
              onclick={() => {
                void updates.installUpdate();
              }}
            >
              <Download size={14} strokeWidth={2} aria-hidden="true" />
              <span>{t("updates.updateAndRestart")}</span>
            </button>
          {:else if updates.primaryAction === "copy-command" && updates.copyCommand}
            <button
              type="button"
              class="inline-flex h-8 items-center justify-center gap-1.5 rounded-md bg-primary px-2.5 text-[0.8rem] font-medium text-primary-foreground transition-colors hover:bg-primary/90 focus:outline-none focus:ring-1 focus:ring-ring"
              onclick={() => {
                void copyUpdateCommand();
              }}
            >
              <Copy size={14} strokeWidth={2} aria-hidden="true" />
              <span>{commandCopied ? t("updates.commandCopied") : t("updates.copyCommand")}</span>
            </button>
          {/if}
          <button
            type="button"
            class={cn(
              "inline-flex h-8 items-center justify-center gap-1.5 rounded-md px-2.5 text-[0.8rem] font-medium transition-colors focus:outline-none focus:ring-1 focus:ring-ring",
              updates.releasePageUrl
                ? "text-popover-foreground hover:bg-accent hover:text-accent-foreground"
                : "pointer-events-none text-muted-foreground/55",
            )}
            disabled={!updates.releasePageUrl}
            onclick={() => {
              void openReleasePage();
            }}
          >
            <ExternalLink size={14} strokeWidth={2} aria-hidden="true" />
            {t("updates.releaseNotes")}
          </button>
        </div>
      {/if}
    </div>

    {#if updates.status === "available"}
      <button
        type="button"
        class="flex h-7 w-7 shrink-0 items-center justify-center rounded-md text-muted-foreground transition-colors hover:bg-accent hover:text-foreground focus:outline-none focus:ring-1 focus:ring-ring"
        onclick={() => updates.dismissPrompt()}
      >
        <span class="sr-only">{t("updates.dismissNotification")}</span>
        <X size={14} strokeWidth={2} aria-hidden="true" />
      </button>
    {/if}
  </div>
{/if}
