<script lang="ts">
  import { onMount } from "svelte";
  import X from "@lucide/svelte/icons/x";
  import {
    clearBrowserPreviewData,
    readPreviewModelOrigins,
    revokePreviewModelOrigin,
    type PreviewModelOriginGrant,
  } from "$lib/api/chat";
  import { chatErrorMessage } from "$lib/chat/error-presentation";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import { getChat } from "$lib/stores/chat.svelte";
  import { SETTINGS_ICON_BUTTON_CLASS, SETTINGS_SECONDARY_BUTTON_CLASS } from "$lib/components/settings/action-buttons";
  import Notice from "$lib/components/ui/Notice.svelte";

  /** Sites Chat browser previews may reach, and the preview data each working folder keeps on this device. */
  const { t } = getLocalization();
  const chat = getChat();
  let siteGrants = $state<PreviewModelOriginGrant[]>([]);
  let siteError = $state<string | null>(null);
  let revoking = $state(false);
  let clearingPreviewData = $state(false);
  let previewDataCleared = $state(false);
  let previewDataError = $state<string | null>(null);

  onMount(() => {
    void loadSiteGrants().catch(() => undefined);
  });

  async function loadSiteGrants(): Promise<void> {
    siteError = null;
    try {
      siteGrants = await readPreviewModelOrigins();
    } catch (cause: unknown) {
      siteError = errorMessage(cause);
      throw cause;
    }
  }

  async function revokeSite(grant: PreviewModelOriginGrant): Promise<void> {
    if (revoking) return;
    revoking = true;
    siteError = null;
    try {
      await revokePreviewModelOrigin(grant.threadId, grant.origin);
      siteGrants = siteGrants.filter((entry) => entry.threadId !== grant.threadId || entry.origin !== grant.origin);
    } catch (cause: unknown) {
      siteError = errorMessage(cause);
    } finally {
      revoking = false;
    }
  }

  async function clearPreviewData(): Promise<void> {
    if (clearingPreviewData) return;
    clearingPreviewData = true;
    previewDataCleared = false;
    previewDataError = null;
    try {
      await clearBrowserPreviewData();
      previewDataCleared = true;
    } catch (cause: unknown) {
      previewDataError = errorMessage(cause);
    } finally {
      clearingPreviewData = false;
    }
  }

  function threadLabel(threadId: string): string {
    return [...chat.activeThreads, ...chat.archivedThreads].find((thread) => thread.id === threadId)?.title || threadId;
  }

  function errorMessage(cause: unknown): string {
    return chatErrorMessage(cause, t("chatErrors.actions.settingsAction"));
  }
</script>

<section class="flex flex-col gap-4">
  <h2 class="px-1 text-[0.866667rem] font-semibold text-foreground">{t("settings.chat.browserPreview.heading")}</h2>
  <div class="flex flex-col gap-3">
    <div class="flex flex-col gap-2 px-1">
      <div>
        <div class="text-[0.866667rem] text-foreground">{t("settings.chat.browserPreview.siteAccessHeading")}</div>
        <div class="mt-0.5 text-[0.8rem] text-muted-foreground">{t("settings.chat.browserPreview.siteAccessDescription")}</div>
      </div>
      {#if siteError}
        <Notice class="text-[0.733333rem]" message={siteError} onRetry={loadSiteGrants} />
      {/if}
      {#if siteGrants.length === 0}
        <p class="text-[0.8rem] text-muted-foreground">{t("settings.chat.browserPreview.siteAccessEmpty")}</p>
      {:else}
        <ul class="flex flex-col">
          {#each siteGrants as grant (`${grant.threadId} ${grant.origin}`)}
            <li class="flex items-center justify-between gap-2 py-1">
              <div class="min-w-0">
                <p class="truncate font-mono text-[0.766667rem] text-foreground">{grant.origin}</p>
                <p class="truncate text-[0.733333rem] text-muted-foreground">{t("settings.chat.browserPreview.siteAccessThread", threadLabel(grant.threadId))}</p>
              </div>
              <button
                type="button"
                class={SETTINGS_ICON_BUTTON_CLASS}
                disabled={revoking}
                aria-label={t("settings.chat.browserPreview.siteAccessRevoke", grant.origin)}
                title={t("settings.chat.browserPreview.siteAccessRevoke", grant.origin)}
                onclick={() => void revokeSite(grant)}
              ><X size={13} /></button>
            </li>
          {/each}
        </ul>
      {/if}
    </div>

    <div class="flex items-center justify-between gap-4 px-1 py-1 max-[480px]:flex-col max-[480px]:items-stretch max-[480px]:gap-2">
      <div class="min-w-0 flex-1">
        <div class="text-[0.866667rem] text-foreground">{t("settings.chat.browserPreview.previewDataHeading")}</div>
        <div class="mt-0.5 text-[0.8rem] text-muted-foreground">
          {previewDataCleared ? t("settings.chat.browserPreview.previewDataCleared") : t("settings.chat.browserPreview.previewDataDescription")}
        </div>
      </div>
      <button type="button" class={SETTINGS_SECONDARY_BUTTON_CLASS} disabled={clearingPreviewData} onclick={() => void clearPreviewData()}>
        {t("settings.chat.browserPreview.previewDataClear")}
      </button>
    </div>
    {#if previewDataError}
      <Notice class="px-1 text-[0.733333rem]" message={previewDataError} />
    {/if}
  </div>
</section>
