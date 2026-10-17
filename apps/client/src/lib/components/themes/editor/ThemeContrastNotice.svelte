<script lang="ts">
  import AlertTriangle from "@lucide/svelte/icons/alert-triangle";
  import ArrowDown from "@lucide/svelte/icons/arrow-down";
  import Wand2 from "@lucide/svelte/icons/wand-2";
  import { getLocalization } from "$lib/i18n/translator.svelte";

  let {
    count,
    onJump,
    onFixAll,
  }: {
    count: number;
    onJump: () => void;
    onFixAll: () => void;
  } = $props();

  const { t } = getLocalization();
</script>

<section
  class="theme-contrast-notice absolute z-30 flex h-10 items-center justify-center gap-2 px-3 surface-floating"
>
  <div class="theme-contrast-message flex shrink-0 items-center gap-2 text-foreground">
    <AlertTriangle
      size={13}
      strokeWidth={2.25}
      class="shrink-0 text-amber-500"
    />
    <span class="font-medium">
      {t("settings.theme.editor.contrastIssue", count)}
    </span>
  </div>
  <div class="theme-contrast-actions flex shrink-0 items-center gap-1.5">
    <button
      type="button"
      onclick={onJump}
      aria-label={t("settings.theme.editor.jumpToNextContrast")}
      title={t("settings.theme.editor.jumpToNextContrastTitle")}
      class="flex h-6 items-center gap-1 rounded-md border border-border bg-card px-2 text-[0.666667rem] font-medium text-foreground transition-colors hover:bg-accent"
    >
      <ArrowDown size={11} strokeWidth={2.25} />
      <span>{t("settings.theme.editor.jumpToNext")}</span>
    </button>
    <button
      type="button"
      onclick={onFixAll}
      aria-label={t("settings.theme.editor.fixAllContrast")}
      title={t("settings.theme.editor.fixAllContrastTitle")}
      class="flex h-6 items-center gap-1 rounded-md border border-primary bg-primary px-2 text-[0.666667rem] font-medium text-primary-foreground transition-colors hover:bg-primary/90"
    >
      <Wand2 size={11} strokeWidth={2.25} />
      <span>{t("settings.theme.editor.fixAll")}</span>
    </button>
  </div>
</section>

<style>
  .theme-contrast-notice {
    bottom: 0.75rem;
    left: 50%;
    max-width: calc(100% - 0.75rem);
    width: max-content;
    transform: translateX(-50%);
  }

  .theme-contrast-message {
    white-space: nowrap;
  }

  @container theme-editor (max-width: 620px) {
    .theme-contrast-notice {
      bottom: 0.75rem;
      height: auto;
      max-width: calc(100% - 0.75rem);
      min-height: 2.5rem;
    }
  }

  @container theme-editor (max-width: 430px) {
    .theme-contrast-notice {
      align-items: center;
      justify-content: center;
      max-width: calc(100% - 0.75rem);
    }
  }

  @container theme-editor (max-width: 380px) {
    .theme-contrast-notice {
      left: 0.5rem;
      right: 0.5rem;
      width: auto;
      max-width: none;
      min-height: 4.25rem;
      padding: 0.625rem;
      flex-wrap: wrap;
      transform: none;
    }

    .theme-contrast-message,
    .theme-contrast-actions {
      width: 100%;
    }

    .theme-contrast-message {
      justify-content: center;
    }

    .theme-contrast-actions button {
      flex: 1 1 0;
      justify-content: center;
      min-width: 0;
    }
  }
</style>
