<script lang="ts">
  import { tick } from "svelte";
  import type { HTMLInputAttributes } from "svelte/elements";
  import Plus from "@lucide/svelte/icons/plus";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import { cn } from "$lib/utils";
  let { label, draft = $bindable(""), active = $bindable(false), disabled = false, inputAttributes, contentColumn, indent, onDraftChange, onActiveChange, onSubmit, onCreate }: {
    label: string;
    draft?: string;
    active?: boolean;
    disabled?: boolean;
    inputAttributes?: HTMLInputAttributes;
    /** Grid column where content starts when the control fills a whole collection row and relies on the row hover. */
    contentColumn?: number;
    /** Start inset of the content, matching the title indent of the nested rows it adds to. */
    indent?: string;
    onDraftChange?: (draft: string) => void;
    onActiveChange?: (active: boolean) => void;
  } & ({ onSubmit: (title: string) => Promise<boolean>; onCreate?: never }
    | { onCreate: () => void; onSubmit?: never }) = $props();
  const { t } = getLocalization();
  const spansRow = $derived(contentColumn !== undefined);
  const contentStyle = $derived([
    spansRow ? `grid-column: ${contentColumn} / -1;` : "",
    indent ? `margin-left: ${indent};` : "",
  ].join(" ") || undefined);
  let input: HTMLInputElement | undefined = $state();
  let trigger: HTMLButtonElement | undefined = $state();
  let pending = $state(false);
  let error = $state<string | null>(null);

  async function begin(): Promise<void> {
    if (onCreate) { onCreate(); return; }
    active = true;
    onActiveChange?.(true);
    await tick();
    input?.focus();
  }

  async function submit(): Promise<void> {
    if (!onSubmit || !draft.trim() || disabled || pending) return;
    pending = true;
    error = null;
    try {
      if (await onSubmit(draft.trim())) { draft = ""; onDraftChange?.(""); }
    } catch (caught) {
      error = caught instanceof Error ? caught.message : String(caught);
    } finally {
      pending = false;
      await tick();
      input?.focus();
    }
  }

  function cancel(): void {
    draft = "";
    active = false;
    error = null;
    onDraftChange?.("");
    onActiveChange?.(false);
    void tick().then(() => trigger?.focus({ preventScroll: true }));
  }
</script>

<div class={spansRow ? "col-span-full grid min-w-0 grid-cols-subgrid self-stretch" : "min-w-0"}>
  {#if active || draft}
    <form class={cn("flex min-h-(--collection-table-row-height) min-w-0 items-center gap-1 rounded-md px-2", !spansRow && "bg-accent/20")} style={contentStyle} onsubmit={(event) => { event.preventDefault(); void submit(); }}>
      <input {...inputAttributes} bind:this={input} value={draft} oninput={(event) => { draft = event.currentTarget.value; onDraftChange?.(draft); }} aria-label={label} placeholder={label} disabled={disabled || pending} class="min-h-8 w-full min-w-0 bg-transparent text-collection text-foreground outline-none" onkeydown={(event) => { event.stopPropagation(); if (event.key === "Escape" && !pending) { event.preventDefault(); cancel(); } }} />
      <button type="submit" class="flex size-8 shrink-0 items-center justify-center rounded-md text-muted-foreground hover:bg-accent hover:text-foreground" disabled={disabled || pending || !draft.trim()} aria-label={label}><Plus class="size-4" /></button>
      <button type="button" class="rounded-md px-2 py-1 text-[0.8rem] text-muted-foreground hover:bg-accent" disabled={pending} onclick={cancel}>{t("common.cancel")}</button>
    </form>
  {:else if spansRow}
    <button bind:this={trigger} type="button" class="col-span-full grid min-h-(--collection-table-row-height) grid-cols-subgrid items-center text-left text-collection text-muted-foreground outline-none hover:text-foreground focus-visible:bg-accent/40 focus-visible:text-foreground" {disabled} onclick={() => { void begin(); }}>
      <span class="flex min-w-0 items-center gap-2 px-2" style={contentStyle}><Plus class="size-4 shrink-0" /><span class="truncate">{label}</span></span>
    </button>
  {:else}
    <button bind:this={trigger} type="button" class="flex min-h-(--collection-table-row-height) w-full items-center gap-2 rounded-md px-2 text-left text-collection text-muted-foreground hover:bg-accent/40 hover:text-foreground" {disabled} onclick={() => { void begin(); }}><Plus class="size-4" />{label}</button>
  {/if}
  {#if error}<p role="alert" class="px-2 py-1 text-[0.8rem] text-destructive" style={contentStyle}>{error}</p>{/if}
</div>
