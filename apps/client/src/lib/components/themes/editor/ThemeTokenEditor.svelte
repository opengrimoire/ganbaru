<script lang="ts">
  import Link2 from "@lucide/svelte/icons/link-2";
  import Pencil from "@lucide/svelte/icons/pencil";
  import RotateCcw from "@lucide/svelte/icons/rotate-ccw";
  import ColorField from "$lib/components/ui/ColorField.svelte";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import { cn } from "$lib/utils";

  let {
    value,
    label,
    scope,
    linked,
    readOnly,
    canReset,
    onChange,
    onReset,
    onIsolate,
    onRelink,
  }: {
    value: string;
    label: string;
    scope: "app" | "calendar";
    linked: boolean;
    readOnly: boolean;
    canReset: boolean;
    onChange: (value: string) => void;
    onReset: () => void;
    onIsolate: () => void;
    onRelink: () => void;
  } = $props();

  const { t } = getLocalization();
  const resetTitle = $derived(
    readOnly
      ? t("settings.theme.editor.builtInReadOnly")
      : linked
        ? t("settings.theme.editor.linkedColorsResetThroughSource")
        : t("settings.theme.editor.originalValue"),
  );
</script>

<div class="theme-token-editor flex items-center gap-1.5" data-token-scope={scope}>
  <ColorField
    {value}
    onChange={(next) => {
      if (!linked) onChange(next);
    }}
    readOnly={readOnly || linked}
    {label}
  />
  <button
    type="button"
    disabled={!canReset}
    onclick={onReset}
    aria-disabled={!canReset}
    aria-label={t("settings.theme.editor.resetOriginal", label)}
    title={canReset ? t("settings.theme.editor.restoreOriginal") : resetTitle}
    class={cn(
      "flex h-6.5 w-6.5 shrink-0 items-center justify-center rounded-md border border-border bg-secondary text-secondary-foreground transition-colors",
      canReset
        ? "hover:bg-accent hover:text-accent-foreground"
        : "cursor-not-allowed opacity-40",
    )}
  >
    <RotateCcw size={11} strokeWidth={2.25} />
  </button>
  {#if linked}
    <button
      type="button"
      onclick={onIsolate}
      disabled={readOnly}
      aria-label={t("settings.theme.editor.isolateEditLabel", label)}
      title={readOnly
        ? t("settings.theme.editor.builtInReadOnly")
        : t("settings.theme.editor.isolateEditTitle")}
      class={cn(
        "theme-token-action flex min-w-27 shrink-0 items-center justify-center gap-1 rounded-md border border-border bg-card px-2 py-1 text-[0.666667rem] font-medium text-muted-foreground transition-colors",
        readOnly
          ? "cursor-not-allowed opacity-60"
          : "hover:border-foreground/30 hover:bg-accent hover:text-foreground",
      )}
    >
      <Pencil size={10} strokeWidth={2.25} />
      <span>{t("settings.theme.editor.isolateEdit")}</span>
    </button>
  {:else}
    <button
      type="button"
      onclick={onRelink}
      disabled={readOnly}
      aria-label={t("settings.theme.editor.linkBackLabel", label)}
      title={readOnly
        ? t("settings.theme.editor.builtInReadOnly")
        : t("settings.theme.editor.linkBackTitle")}
      class={cn(
        "theme-token-action flex min-w-27 shrink-0 items-center justify-center gap-1 rounded-md border border-border bg-card px-2 py-1 text-[0.666667rem] font-medium text-muted-foreground transition-colors",
        readOnly
          ? "cursor-not-allowed opacity-60"
          : "hover:border-foreground/30 hover:bg-accent hover:text-foreground",
      )}
    >
      <Link2 size={10} strokeWidth={2.25} />
      <span>{t("settings.theme.editor.linkBack")}</span>
    </button>
  {/if}
</div>

<style>
  @container theme-editor (max-width: 430px) {
    .theme-token-editor { flex-wrap: wrap; justify-content: flex-end; }
    .theme-token-action { min-width: 6.5rem; }
  }
</style>
