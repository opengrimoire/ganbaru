<script lang="ts">
  import { tick } from "svelte";
  import Check from "@lucide/svelte/icons/check";
  import Tag from "@lucide/svelte/icons/tag";
  import { FLOATING_WIDTH } from "$lib/components/ui/floating-width";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import { activateModalFocus } from "$lib/modal-focus";
  import type { QuickNoteTag } from "$lib/quick-notes/types";
  import { getMobileBackStack } from "$lib/stores/mobile-back-stack.svelte";
  import { portal } from "$lib/utils/portal";
  import { scrollEdgeFadeAction } from "$lib/utils/scroll-edge-fade";

  let {
    tagId,
    tags,
    onSelect,
    buttonClass = "",
    mobileLayout = false,
  }: {
    tagId: string | null;
    tags: readonly QuickNoteTag[];
    onSelect: (tagId: string | null) => void;
    buttonClass?: string;
    mobileLayout?: boolean;
  } = $props();

  const { t } = getLocalization();
  const mobileBackStack = getMobileBackStack();
  let open = $state(false);
  let button = $state<HTMLButtonElement | null>(null);
  let firstOption = $state<HTMLButtonElement | null>(null);
  let picker = $state<HTMLDivElement | null>(null);
  let left = $state(8);
  let top = $state(8);

  async function openPicker(): Promise<void> {
    if (!button) return;
    const rect = button.getBoundingClientRect();
    const width = FLOATING_WIDTH.sm;
    const height = Math.min(300, 44 + (tags.length + 1) * 34);
    left = Math.min(Math.max(8, rect.left), Math.max(8, window.innerWidth - width - 8));
    top = rect.bottom + height + 8 <= window.innerHeight
      ? rect.bottom + 4
      : Math.max(8, rect.top - height - 4);
    open = true;
    await tick();
    firstOption?.focus();
  }

  function choose(next: string | null): void {
    onSelect(next);
    closePicker();
  }

  function closePicker(): void {
    open = false;
    void tick().then(() => button?.focus());
  }

  function handlePickerKeydown(event: KeyboardEvent): void {
    if (event.key !== "Escape") return;
    event.preventDefault();
    event.stopPropagation();
    closePicker();
  }

  const pickerStyle = $derived(mobileLayout
    ? "left: calc(var(--visual-viewport-offset-left) + var(--safe-area-left) + 0.5rem); right: calc(var(--safe-area-right) + 0.5rem); bottom: calc(var(--keyboard-inset) + var(--safe-area-bottom) + 0.5rem); max-width: 32rem; max-height: min(60vh, 24rem, calc(var(--visual-viewport-height) - var(--safe-area-top) - var(--safe-area-bottom) - 1rem)); margin-inline: auto;"
    : `left: ${left}px; top: ${top}px;`);

  $effect(() => {
    if (!open) return;
    const deactivateBack = mobileLayout
      ? mobileBackStack.activate({ handle: closePicker })
      : () => undefined;
    const deactivateFocus = picker
      ? activateModalFocus(picker, firstOption)
      : () => undefined;
    window.addEventListener("keydown", handlePickerKeydown, true);
    return () => {
      deactivateBack();
      deactivateFocus();
      window.removeEventListener("keydown", handlePickerKeydown, true);
    };
  });
</script>

<button
  bind:this={button}
  type="button"
  class={buttonClass}
  aria-label={t("quickNotes.tag.assign")}
  title={t("quickNotes.tag.assign")}
  aria-haspopup="dialog"
  aria-expanded={open}
  onclick={() => { if (open) closePicker(); else void openPicker(); }}
>
  <Tag class="size-4" strokeWidth={1.5} aria-hidden="true" />
</button>

{#if open}
  <button
    use:portal
    type="button"
    class="fixed inset-0 z-90 cursor-default"
    aria-label={t("common.close")}
    onclick={closePicker}
  ></button>
  <div
    bind:this={picker}
    use:portal
    class={mobileLayout ? "surface-floating fixed z-100 flex flex-col overflow-hidden" : "surface-floating fixed z-100 flex max-h-75 w-floating-sm flex-col overflow-hidden"}
    style={pickerStyle}
    role="dialog"
    aria-modal="true"
    tabindex="-1"
    aria-label={t("quickNotes.tag.assign")}
  >
    <div use:scrollEdgeFadeAction class="surface-floating-body min-h-0 overflow-y-auto overscroll-contain">
      <button
        bind:this={firstOption}
        type="button"
        class="menu-item"
        onclick={() => choose(null)}
      >
        <span class="min-w-0 flex-1 truncate">{t("quickNotes.tag.none")}</span>
        {#if tagId === null}<Check class="size-3.5 shrink-0" strokeWidth={1.5} aria-hidden="true" />{/if}
      </button>
      {#each tags as tag}
        <button
          type="button"
          class="menu-item"
          onclick={() => choose(tag.id)}
        >
          <span class="min-w-0 flex-1 truncate">{tag.name}</span>
          {#if tagId === tag.id}<Check class="size-3.5 shrink-0" strokeWidth={1.5} aria-hidden="true" />{/if}
        </button>
      {/each}
    </div>
  </div>
{/if}
