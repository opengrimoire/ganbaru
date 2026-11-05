<script lang="ts">
  import ChevronRight from "@lucide/svelte/icons/chevron-right";
  import { getLocalization } from "$lib/i18n/translator.svelte";

  /** Where linking lives in Settings, as a breadcrumb chip inside a step's sentence. */
  let {
    toLinkedDevices = false,
  }: {
    /** Whether the path ends at the Linked devices section instead of the Data page. */
    toLinkedDevices?: boolean;
  } = $props();

  const { t } = getLocalization();
</script>

<span class="ms-0.5 inline-flex items-center gap-0.5 rounded-md bg-muted px-1.5 py-px align-baseline text-[0.8rem] font-medium whitespace-nowrap">
  {t("settings.title")}<ChevronRight size={12} class="text-muted-foreground rtl:-scale-x-100" aria-hidden="true" />{t("settings.section.data")}
  {#if toLinkedDevices}<ChevronRight size={12} class="text-muted-foreground rtl:-scale-x-100" aria-hidden="true" />{t("vaultHandoff.heading")}{/if}
</span>
