<script lang="ts">
  import AlertTriangle from "@lucide/svelte/icons/alert-triangle";
  import Wand2 from "@lucide/svelte/icons/wand-2";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import type { ThemeSources } from "$lib/themes";
  import { cn } from "$lib/utils";
  import type { GroupSourcePairRow } from "./model";
  import type { PairContrast } from "./contrast-controller.svelte";
  import ThemeSourceEditor from "./ThemeSourceEditor.svelte";

  let {
    row,
    contrast,
    contrastTitle,
    pairKey,
    readOnly,
    sourceValue,
    canResetSource,
    onSetSource,
    onResetSource,
    onAutoFix,
  }: {
    row: GroupSourcePairRow;
    contrast: PairContrast;
    contrastTitle: string;
    pairKey: string;
    readOnly: boolean;
    sourceValue: (key: keyof ThemeSources) => string;
    canResetSource: (key: keyof ThemeSources) => boolean;
    onSetSource: (key: keyof ThemeSources, value: string) => void;
    onResetSource: (key: keyof ThemeSources) => void;
    onAutoFix: () => void;
  } = $props();

  const { t } = getLocalization();
</script>

<div
  data-pair-key={pairKey}
  class="theme-pair-row flex flex-wrap items-start justify-between gap-x-4 gap-y-2 px-1 py-2.5"
>
  <div class="min-w-0 flex-1">
    <div class="flex items-center gap-1.5">
      <span class="text-[0.866667rem] font-semibold text-foreground">{row.title}</span>
      {#if !contrast.passes}
        <button
          type="button"
          onclick={onAutoFix}
          disabled={readOnly}
          aria-label={readOnly
            ? t(
                "settings.theme.editor.contrastReadOnlyLabel",
                row.title,
                contrast.ratio.toFixed(2),
              )
            : t("settings.theme.editor.contrastAutoFixLabel", row.title)}
          title={contrastTitle}
          class={cn(
            "flex items-center gap-1 rounded px-1 py-0.5 text-[0.666667rem] font-medium text-amber-700 transition-colors dark:text-amber-400",
            readOnly ? "cursor-not-allowed opacity-75" : "hover:bg-amber-500/10",
          )}
        >
          <AlertTriangle size={11} strokeWidth={2.25} />
          <span>{contrast.ratio.toFixed(1)}:1</span>
          {#if !readOnly}<Wand2 size={10} strokeWidth={2.25} />{/if}
        </button>
      {/if}
    </div>
    <div class="text-[0.733333rem] text-muted-foreground">{row.description}</div>
  </div>
  <div class="theme-pair-controls flex shrink-0 flex-col items-end gap-2">
    {#each [
      {
        label: t("settings.theme.editor.backgroundShort"),
        controlLabel: t("settings.theme.editor.backgroundControl", row.title),
        source: row.bgSource,
      },
      {
        label: t("settings.theme.editor.textShort"),
        controlLabel: t("settings.theme.editor.textControl", row.title),
        source: row.fgSource,
      },
    ] as item (item.source)}
      <div class="theme-pair-control-line flex items-center gap-1.5">
        <span class="theme-pair-label w-8.5 text-right text-[0.666667rem] font-medium uppercase tracking-wide text-muted-foreground">
          {item.label}
        </span>
        <ThemeSourceEditor
          value={sourceValue(item.source)}
          label={item.controlLabel}
          {readOnly}
          canReset={canResetSource(item.source)}
          onChange={(value) => onSetSource(item.source, value)}
          onReset={() => onResetSource(item.source)}
        />
      </div>
    {/each}
  </div>
</div>

<style>
  @container theme-editor (max-width: 620px) {
    .theme-pair-controls { width: 100%; align-items: stretch; }
    .theme-pair-control-line { justify-content: space-between; }
  }
  @container theme-editor (max-width: 430px) {
    .theme-pair-label { width: auto; min-width: 2.25rem; }
  }
</style>
