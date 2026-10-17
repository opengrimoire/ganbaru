<script lang="ts">
  import { onMount } from "svelte";
  import ExternalLink from "@lucide/svelte/icons/external-link";
  import RefreshCw from "@lucide/svelte/icons/refresh-cw";
  import { openUrl } from "@tauri-apps/plugin-opener";
  import { BUILD_REF, GITHUB_REPOSITORY } from "$lib/build-info";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import { formatDateTime } from "$lib/i18n/formatters";
  import { latestReleasePageUrl } from "$lib/stores/update-policy";
  import { getMobileUpdateManager } from "$lib/stores/mobile-updates.svelte";
  import { cn } from "$lib/utils";
  import Notice from "$lib/components/ui/Notice.svelte";

  const { t, locale } = getLocalization();
  const releasePageUrl = latestReleasePageUrl(GITHUB_REPOSITORY);
  const updates = getMobileUpdateManager();
  const actionButtonClass =
    "inline-flex h-7 shrink-0 items-center justify-center gap-1.5 rounded-md border border-border bg-card px-2.5 text-[0.8rem] font-medium transition-colors focus:outline-none focus-visible:ring-1 focus-visible:ring-ring disabled:pointer-events-none disabled:opacity-55";
  const primaryButtonClass =
    `${actionButtonClass} border-primary bg-primary text-primary-foreground hover:bg-primary/90`;

  let openPageError = $state<string | null>(null);

  async function openReleasePage(): Promise<void> {
    const url = updates.latestReleaseUrl ?? releasePageUrl;
    if (!url) return;
    openPageError = null;
    try {
      await openUrl(url);
    } catch (cause: unknown) {
      openPageError = cause instanceof Error ? cause.message : String(cause);
    }
  }

  async function downloadApk(): Promise<void> {
    if (!updates.latestApkUrl) return;
    openPageError = null;
    try {
      await openUrl(updates.latestApkUrl);
    } catch (cause: unknown) {
      openPageError = cause instanceof Error ? cause.message : String(cause);
    }
  }

  function formatPublishedAt(value: string): string {
    const date = Date.parse(value);
    if (!Number.isFinite(date)) return value;
    return formatDateTime(locale, new Date(date), {
      dateStyle: "medium",
      timeStyle: "short",
    });
  }

  onMount(() => {
    void updates.checkForUpdates();
  });
</script>

<div class="flex flex-col gap-6">
  <section class="flex flex-col gap-4">
    <h2 class="px-1 text-[0.866667rem] font-semibold text-foreground">
      {t("settings.updates.buildHeading")}
    </h2>

    <div class="flex flex-col gap-3">
      <div class="flex items-start justify-between gap-4 px-1 py-1">
        <div class="min-w-0 flex-1">
          <div class="text-[0.866667rem] text-foreground">
            {t("settings.updates.installedBuild")}
          </div>
          <div class="mt-0.5 wrap-break-word text-[0.8rem] leading-5 text-muted-foreground">
            {BUILD_REF}
          </div>
          {#if updates.installedVersion}
            <div class="mt-1 text-[0.8rem] leading-5 text-muted-foreground">
              {t("settings.updates.currentReleaseVersion", updates.installedVersion)}
            </div>
          {/if}
          <div class="mt-1 text-[0.8rem] leading-5 text-muted-foreground">
            {t("settings.updates.updateStatus")}: {updates.statusCopy}
          </div>
          {#if updates.publishedAt && updates.status === "available"}
            <div class="mt-1 text-[0.8rem] leading-5 text-muted-foreground">
              {t("mobile.settings.androidLatestPublished", formatPublishedAt(updates.publishedAt))}
            </div>
          {/if}
        </div>

        <button
          type="button"
          class={primaryButtonClass}
          disabled={updates.status === "checking"}
          onclick={() => {
            void updates.checkForUpdates({ force: true });
          }}
        >
          <RefreshCw
            size={14}
            strokeWidth={1.9}
            class={cn("shrink-0", updates.status === "checking" ? "animate-spin" : "")}
          />
          <span>{t("settings.updates.checkForUpdates")}</span>
        </button>
      </div>

      {#if updates.status === "error"}
        <Notice class="px-1 text-[0.8rem]" message={updates.statusCopy} />
      {/if}
    </div>
  </section>

  {#if updates.status === "available"}
    <section class="flex flex-col gap-4">
      <h2 class="px-1 text-[0.866667rem] font-semibold text-foreground">{t("settings.updates.availableUpdateHeading")}</h2>

      <div class="flex flex-col gap-3">
        <div class="flex items-start justify-between gap-4 px-1 py-1">
          <div class="min-w-0 flex-1">
            <div class="text-[0.866667rem] text-foreground">
              {t("settings.updates.version", updates.latestVersion ?? "unknown")}
            </div>
            <div class="mt-0.5 text-[0.8rem] leading-5 text-muted-foreground">
              {t("settings.updates.manualInstallUpdateDescriptionMobile")}
            </div>
          </div>
          <button
            type="button"
            class={primaryButtonClass}
            disabled={!updates.latestApkUrl && !updates.latestReleaseUrl && !releasePageUrl}
            onclick={() => {
              if (updates.latestApkUrl) {
                void downloadApk();
              } else {
                void openReleasePage();
              }
            }}
          >
            <ExternalLink size={14} strokeWidth={1.9} class="shrink-0" />
            <span>
              {updates.latestApkUrl
                ? t("mobile.settings.downloadApk")
                : t("mobile.settings.viewReleases")}
            </span>
          </button>
        </div>
      </div>
    </section>
  {/if}

  {#if updates.status !== "available"}
    <section class="flex flex-col gap-4">
      <div class="flex items-start justify-between gap-4 px-1 py-1">
        <div class="min-w-0 flex-1">
          <div class="text-[0.866667rem] text-foreground">
            {t("settings.updates.releaseNotes")}
          </div>
          <div class="mt-0.5 text-[0.8rem] leading-5 text-muted-foreground">
            {t("mobile.settings.androidUpdatesDescription")}
          </div>
        </div>
        <button
          type="button"
          class={actionButtonClass}
          disabled={!updates.latestReleaseUrl && !releasePageUrl}
          onclick={() => {
            void openReleasePage();
          }}
        >
          <ExternalLink size={14} strokeWidth={1.9} class="shrink-0" />
          <span>{t("mobile.settings.viewReleases")}</span>
        </button>
      </div>
    </section>
  {/if}

  {#if openPageError}
    <Notice class="px-1 text-[0.8rem]" message={openPageError} />
  {/if}
</div>
