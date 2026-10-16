<script lang="ts">
  import { tick } from "svelte";
  import Plus from "@lucide/svelte/icons/plus";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import { QUICK_NOTE_TAG_LIMIT } from "$lib/quick-notes/types";
  import { getMobileBackStack } from "$lib/stores/mobile-back-stack.svelte";
  import QuickNoteTagNameInput from "./QuickNoteTagNameInput.svelte";

  let {
    tagCount,
    onCreate,
    mobileLayout = false,
  }: {
    tagCount: number;
    onCreate: (name: string) => Promise<void>;
    mobileLayout?: boolean;
  } = $props();

  const { t } = getLocalization();
  const mobileBackStack = getMobileBackStack();
  let creating = $state(false);
  let input = $state<HTMLInputElement | null>(null);
  let createButton = $state<HTMLButtonElement | null>(null);
  let name = $state("");
  let busy = $state(false);
  let error = $state("");

  async function begin(): Promise<void> {
    if (tagCount >= QUICK_NOTE_TAG_LIMIT) return;
    creating = true;
    name = "";
    error = "";
    await tick();
    input?.focus();
  }

  async function create(): Promise<void> {
    const value = name.trim();
    if (!value || busy || tagCount >= QUICK_NOTE_TAG_LIMIT) return;
    busy = true;
    error = "";
    try {
      await onCreate(value);
      name = "";
      creating = false;
    } catch {
      error = t("quickNotes.tag.saveFailed");
    } finally {
      busy = false;
    }
    // The input is disabled while saving, so it can take focus back only once busy clears.
    if (!error) return;
    await tick();
    input?.focus();
  }

  function cancel(restoreFocus = false): void {
    if (busy) return;
    creating = false;
    name = "";
    error = "";
    if (restoreFocus) void tick().then(() => createButton?.focus());
  }

  $effect(() => {
    if (!mobileLayout || !creating) return;
    return mobileBackStack.activate({ handle: () => cancel(true) });
  });
</script>

<div class="shrink-0" data-quick-note-tag-creator>
  {#if creating}
    <QuickNoteTagNameInput
      bind:value={name}
      bind:input
      label={t("quickNotes.tag.namePlaceholder")}
      placeholder={t("quickNotes.tag.namePlaceholder")}
      {error}
      {busy}
      {mobileLayout}
      onInput={() => { error = ""; }}
      onSubmit={() => void create()}
      onCancel={() => cancel(true)}
      onBlur={() => { if (!busy) cancel(); }}
    >
      {#snippet leading()}<Plus class="size-3.5 shrink-0 text-muted-foreground" strokeWidth={1.5} aria-hidden="true" />{/snippet}
    </QuickNoteTagNameInput>
  {:else if tagCount < QUICK_NOTE_TAG_LIMIT}
    <button
      bind:this={createButton}
      type="button"
      class={mobileLayout ? "flex min-h-12 shrink-0 items-center gap-1 rounded-xl px-4 text-sm text-muted-foreground transition-colors active:bg-accent active:text-foreground" : "flex h-7 shrink-0 items-center gap-1 rounded-md px-2 text-xs text-muted-foreground transition-colors hover:bg-accent hover:text-foreground"}
      aria-label={t("quickNotes.tag.add")}
      onclick={() => void begin()}
    >
      <Plus class="size-3.5" strokeWidth={1.5} />
      <span class="max-[360px]:hidden">{t("quickNotes.tag.add")}</span>
    </button>
  {/if}
</div>
