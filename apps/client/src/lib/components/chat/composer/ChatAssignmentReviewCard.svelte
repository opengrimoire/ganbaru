<script lang="ts">
  import { tick } from "svelte";
  import { getLocalization } from "$lib/i18n/translator.svelte";

  /**
   * Accept and Request changes for work that waits for review in a reply thread. The handlers report their own
   * errors and resolve whether the review was recorded; the card keeps both actions disabled while one runs and
   * keeps the note when it was not sent. The note and whether it is open are bound to the parent, which keeps a
   * typed note when the card closes before it is sent.
   */
  let {
    onAccept,
    onRequestChanges,
    note = $bindable(""),
    composingNote = $bindable(false),
  }: {
    onAccept: () => Promise<boolean>;
    /** Posts the trimmed note as the changes to make. */
    onRequestChanges: (note: string) => Promise<boolean>;
    /** The Request changes note being typed. */
    note?: string;
    /** Whether the note field is open. */
    composingNote?: boolean;
  } = $props();

  const localization = getLocalization();
  const { t } = localization;

  let busy = $state(false);
  let noteElement: HTMLTextAreaElement | undefined = $state();
  const trimmedNote = $derived(note.trim());

  async function run(action: () => Promise<unknown>): Promise<void> {
    if (busy) return;
    busy = true;
    try {
      await action();
    } finally {
      busy = false;
    }
  }

  async function openNote(): Promise<void> {
    composingNote = true;
    await tick();
    noteElement?.focus();
  }

  function closeNote(): void {
    composingNote = false;
    note = "";
  }

  function sendNote(): Promise<void> {
    const text = trimmedNote;
    if (!text) return Promise.resolve();
    return run(async () => {
      if (await onRequestChanges(text)) closeNote();
    });
  }

  function handleNoteKeydown(event: KeyboardEvent): void {
    if (event.key === "Escape") {
      event.preventDefault();
      closeNote();
    } else if (event.key === "Enter" && (event.metaKey || event.ctrlKey)) {
      event.preventDefault();
      void sendNote();
    }
  }
</script>

<section class="review-card" aria-label={t("chat.organization.reviewReady")} aria-busy={busy || undefined}>
  {#if composingNote}
    <textarea
      bind:this={noteElement}
      bind:value={note}
      class="field review-note"
      rows="3"
      aria-label={t("chat.organization.reviewNoteLabel")}
      placeholder={t("chat.organization.reviewNotePlaceholder")}
      disabled={busy}
      onkeydown={handleNoteKeydown}
    ></textarea>
    <div class="review-actions">
      <button type="button" disabled={busy} onclick={closeNote}>{t("chat.organization.reviewCancel")}</button>
      <button type="button" class="primary" disabled={busy || !trimmedNote} onclick={() => { void sendNote(); }}>{t("chat.organization.reviewSend")}</button>
    </div>
  {:else}
    <p>{t("chat.organization.reviewReady")}</p>
    <div class="review-actions">
      <button type="button" disabled={busy} onclick={() => { void openNote(); }}>{t("chat.organization.reviewRequestChanges")}</button>
      <button type="button" class="primary" disabled={busy} onclick={() => { void run(onAccept); }}>{t("chat.organization.reviewAccept")}</button>
    </div>
  {/if}
</section>

<style>
  .review-card { display:grid; gap:0.35rem; margin:0 0.35rem 0.35rem; border-radius:0.65rem; background:color-mix(in srgb,var(--accent) 58%,transparent); padding:0.35rem 0.4rem 0.35rem 0.55rem; color:var(--muted-foreground); }
  .review-card:not(:has(.review-note)) { grid-template-columns:minmax(0,1fr) auto; align-items:center; }
  .review-card p { margin:0; min-width:0; font-size:calc(0.7rem * var(--type-scale)); }
  .review-note { width:100%; resize:vertical; font-size:calc(0.72rem * var(--type-scale)); }
  .review-actions { display:flex; flex-wrap:wrap; justify-content:flex-end; gap:0.3rem; }
  .review-actions button { min-height:1.75rem; border-radius:0.4rem; padding:0.2rem 0.5rem; font-size:calc(0.68rem * var(--type-scale)); font-weight:500; color:var(--foreground); }
  .review-actions button:hover:not(:disabled) { background:color-mix(in srgb,var(--foreground) 8%,transparent); }
  .review-actions button.primary { background:var(--primary); color:var(--primary-foreground); }
  .review-actions button.primary:hover:not(:disabled) { background:color-mix(in srgb,var(--primary) 88%,var(--foreground)); }
  .review-actions button:disabled { opacity:0.5; }
</style>
