<script lang="ts">
  import ChevronRight from "@lucide/svelte/icons/chevron-right";
  import { getLocalization } from "$lib/i18n/translator.svelte";

  /** Where something lives in Settings, as a breadcrumb chip inside a sentence: Settings, then each label of `trail`. */
  let {
    trail,
  }: {
    /** The localized labels after Settings, outermost first. */
    trail: readonly string[];
  } = $props();

  const { t } = getLocalization();
</script>

<span class="ms-0.5 inline-flex items-center gap-0.5 rounded-md bg-muted px-1.5 py-px align-baseline text-[0.8rem] font-medium whitespace-nowrap">
  {t("settings.title")}{#each trail as label, index (index)}<ChevronRight size={12} class="text-muted-foreground rtl:-scale-x-100" aria-hidden="true" />{label}{/each}
</span>
