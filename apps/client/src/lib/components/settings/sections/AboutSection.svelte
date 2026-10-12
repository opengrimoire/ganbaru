<script lang="ts">
  import ExternalLink from "@lucide/svelte/icons/external-link";
  import AboutAcknowledgments from "./AboutAcknowledgments.svelte";
  import { openUrl } from "@tauri-apps/plugin-opener";
  import { getLocalization } from "$lib/i18n/translator.svelte";

  const LICENSE_URL = "https://github.com/opengrimoire/ganbaru-ai/blob/main/LICENSE";
  const actionButtonClass =
    "inline-flex h-7 shrink-0 items-center justify-center gap-1.5 rounded-md border border-border bg-card px-2.5 text-[0.8rem] font-medium text-foreground transition-colors hover:bg-accent focus:outline-none focus-visible:ring-1 focus-visible:ring-ring dark:bg-transparent";
  const { t } = getLocalization();

  async function openExternalUrl(url: string): Promise<void> {
    try {
      await openUrl(url);
    } catch (error: unknown) {
      console.warn("Failed to open external URL:", error);
    }
  }

</script>

<div class="flex flex-col gap-6">
  <section class="flex flex-col gap-4">
    <h2 class="px-1 text-[0.866667rem] font-semibold text-foreground">{t("settings.about.licenseHeading")}</h2>

    <div class="flex flex-col gap-3">
      <div class="flex items-start justify-between gap-4 px-1 py-1 max-[480px]:flex-col max-[480px]:items-stretch max-[480px]:gap-2">
        <div class="min-w-0 flex-1">
          <div class="text-[0.866667rem] text-foreground">{t("settings.about.licenseName")}</div>
          <div class="mt-0.5 text-[0.8rem] leading-5 text-muted-foreground">
            {t("settings.about.licenseDescription")}
          </div>
        </div>
        <button
          type="button"
          class={actionButtonClass}
          onclick={() => {
            void openExternalUrl(LICENSE_URL);
          }}
        >
          <span>{t("settings.about.viewLicense")}</span>
          <ExternalLink size={13} strokeWidth={2.25} aria-hidden="true" />
        </button>
      </div>

      <div class="px-1 py-1">
        <div class="text-[0.866667rem] text-foreground">{t("settings.about.summaryHeading")}</div>
        <div class="mt-0.5 text-[0.8rem] leading-5 text-muted-foreground">
          {t("settings.about.summaryLead")} {t("settings.about.summaryDetails")}
        </div>
      </div>
    </div>
  </section>

  <div class="h-px shrink-0 scale-y-50 bg-border" aria-hidden="true"></div>

  <AboutAcknowledgments />
</div>
