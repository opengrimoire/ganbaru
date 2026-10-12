<script lang="ts">
  import AlertTriangle from "@lucide/svelte/icons/alert-triangle";
  import Wand2 from "@lucide/svelte/icons/wand-2";
  import { getLocalization } from "$lib/i18n/translator.svelte";

  let {
    savedVersion,
    currentVersion,
    onDismiss,
    onRebake,
  }: {
    savedVersion: number;
    currentVersion: number;
    onDismiss: () => void;
    onRebake: () => void;
  } = $props();

  const { t } = getLocalization();
</script>

<section
  class="flex flex-wrap items-start justify-between gap-2 rounded-lg border border-amber-400/60 bg-amber-50 px-3 py-2 text-[0.8rem] dark:bg-amber-950/30"
>
  <div class="flex min-w-0 flex-1 items-start gap-2 text-amber-800 dark:text-amber-300">
    <AlertTriangle size={13} strokeWidth={2.25} class="mt-0.5 shrink-0" />
    <span class="leading-snug">
      {t("settings.theme.rebakeMessage", savedVersion, currentVersion)}
    </span>
  </div>
  <div class="flex shrink-0 items-center gap-1.5">
    <button
      type="button"
      onclick={onDismiss}
      class="flex items-center gap-1 rounded-md border border-border bg-card px-2 py-1 text-[0.733333rem] font-medium text-foreground transition-colors hover:bg-accent"
    >
      {t("settings.theme.maybeLater")}
    </button>
    <button
      type="button"
      onclick={onRebake}
      class="flex items-center gap-1 rounded-md border border-primary bg-primary px-2 py-1 text-[0.733333rem] font-medium text-primary-foreground transition-colors hover:bg-primary/90"
    >
      <Wand2 size={11} strokeWidth={2.25} />
      <span>{t("settings.theme.rebake")}</span>
    </button>
  </div>
</section>
