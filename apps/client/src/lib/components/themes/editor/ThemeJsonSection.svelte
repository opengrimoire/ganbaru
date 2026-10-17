<script lang="ts">
  import Copy from "@lucide/svelte/icons/copy";
  import Download from "@lucide/svelte/icons/download";
  import LoaderCircle from "@lucide/svelte/icons/loader-circle";
  import RotateCcw from "@lucide/svelte/icons/rotate-ccw";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import { cn } from "$lib/utils";

  let {
    isBuiltin,
    jsonDraft,
    jsonDirty,
    jsonErrors,
    jsonNotice,
    jsonSaving,
    fileSaveAvailable,
    onCopy,
    onSave,
    onApply,
    onReset,
    onInput,
  }: {
    isBuiltin: boolean;
    jsonDraft: string;
    jsonDirty: boolean;
    jsonErrors: string[];
    jsonNotice: string | undefined;
    jsonSaving: boolean;
    fileSaveAvailable: boolean;
    onCopy: () => void;
    onSave: () => void;
    onApply: () => void;
    onReset: () => void;
    onInput: (event: Event) => void;
  } = $props();

  const { t } = getLocalization();
</script>

<section class="flex flex-col">
  <header class="px-1 py-2.5">
    <h2 class="text-[0.866667rem] font-semibold text-foreground">{t("settings.theme.jsonSchema")}</h2>
    <div class="text-[0.733333rem] text-muted-foreground">
      {isBuiltin
        ? t("settings.theme.jsonReadonly")
        : t("settings.theme.jsonEditable")}
    </div>
  </header>
  <div class="flex flex-col gap-2">
    <textarea
      value={jsonDraft}
      oninput={isBuiltin ? undefined : onInput}
      readonly={isBuiltin}
      spellcheck={false}
      rows={12}
      class="field w-full resize-y p-2 text-[0.733333rem] text-foreground"
    ></textarea>
    {#if jsonErrors.length > 0}
      <ul
        class="flex flex-col gap-0.5 rounded-md border border-destructive/40 bg-destructive/10 p-2 text-[0.733333rem] text-destructive"
      >
        {#each jsonErrors as err}
          <li>{err}</li>
        {/each}
      </ul>
    {/if}
    <div class="theme-json-actions flex flex-wrap items-center justify-between gap-2">
      <div class="theme-json-action-group flex flex-wrap items-center gap-1.5">
        <button
          type="button"
          onclick={onCopy}
          class="flex items-center gap-1.5 rounded-md border border-border bg-card px-2.5 py-1 text-[0.733333rem] text-foreground transition-colors hover:bg-accent"
        >
          <Copy size={11} strokeWidth={2.25} />
          <span>{t("settings.theme.copyJson")}</span>
        </button>
        {#if fileSaveAvailable}
          <button
            type="button"
            onclick={onSave}
            disabled={jsonSaving}
            aria-busy={jsonSaving}
            class="flex items-center gap-1.5 rounded-md border border-border bg-card px-2.5 py-1 text-[0.733333rem] text-foreground transition-colors hover:bg-accent disabled:cursor-wait disabled:opacity-60"
          >
            {#if jsonSaving}
              <LoaderCircle
                size={11}
                strokeWidth={2.25}
                class="animate-spin motion-reduce:animate-none"
              />
              <span>{t("settings.theme.editor.jsonSaving")}</span>
            {:else}
              <Download size={11} strokeWidth={2.25} />
              <span>{t("settings.theme.saveToFile")}</span>
            {/if}
          </button>
        {/if}
      </div>
      {#if !isBuiltin}
        <div class="theme-json-action-group flex flex-wrap items-center gap-1.5">
          <button
            type="button"
            onclick={onReset}
            disabled={!jsonDirty}
            class={cn(
              "flex items-center gap-1.5 rounded-md border border-border bg-card px-2.5 py-1 text-[0.733333rem] transition-colors",
              jsonDirty
                ? "text-foreground hover:bg-accent"
                : "cursor-not-allowed text-muted-foreground opacity-60",
            )}
          >
            <RotateCcw size={11} strokeWidth={2.25} />
            <span>{t("settings.theme.discardEdits")}</span>
          </button>
          <button
            type="button"
            onclick={onApply}
            disabled={!jsonDirty}
            class={cn(
              "rounded-md border border-border px-3 py-1 text-[0.733333rem] font-medium transition-colors",
              jsonDirty
                ? "bg-primary text-primary-foreground hover:bg-primary/90"
                : "cursor-not-allowed bg-card text-muted-foreground opacity-60",
            )}
          >
            {t("settings.theme.applyChanges")}
          </button>
        </div>
      {/if}
    </div>
    {#if jsonNotice}
      <div class="text-[0.733333rem] text-muted-foreground">{jsonNotice}</div>
    {/if}
  </div>
</section>

<style>
  @container theme-editor (max-width: 620px) {
    .theme-json-actions {
      align-items: stretch;
      flex-direction: column;
    }

    .theme-json-action-group {
      justify-content: flex-end;
    }
  }
</style>
