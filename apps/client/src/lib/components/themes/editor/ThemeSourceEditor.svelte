<script lang="ts">
  import RotateCcw from "@lucide/svelte/icons/rotate-ccw";
  import ColorField from "$lib/components/ui/ColorField.svelte";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import { cn } from "$lib/utils";

  let {
    value,
    label,
    readOnly,
    canReset,
    onChange,
    onReset,
  }: {
    value: string;
    label: string;
    readOnly: boolean;
    canReset: boolean;
    onChange: (value: string) => void;
    onReset: () => void;
  } = $props();

  const { t } = getLocalization();
</script>

<div class="theme-source-editor flex items-center gap-1.5">
  <ColorField {value} onChange={onChange} {readOnly} {label} />
  <button
    type="button"
    disabled={!canReset}
    onclick={onReset}
    aria-disabled={!canReset}
    aria-label={t("settings.theme.editor.resetOriginal", label)}
    title={canReset
      ? t("settings.theme.editor.restoreOriginal")
      : readOnly
        ? t("settings.theme.editor.builtInReadOnly")
        : t("settings.theme.editor.originalValue")}
    class={cn(
      "flex h-6.5 w-6.5 shrink-0 items-center justify-center rounded-md border border-border bg-secondary text-secondary-foreground transition-colors",
      canReset
        ? "hover:bg-accent hover:text-accent-foreground"
        : "cursor-not-allowed opacity-40",
    )}
  >
    <RotateCcw size={11} strokeWidth={2.25} />
  </button>
  <div class="theme-token-action-spacer min-w-27 shrink-0" aria-hidden="true"></div>
</div>

<style>
  @container theme-editor (max-width: 430px) {
    .theme-source-editor { flex-wrap: wrap; justify-content: flex-end; }
    .theme-token-action-spacer { display: none; }
  }
</style>
