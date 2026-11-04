<script lang="ts">
  import SyncConflictCard from "$lib/components/sync/SyncConflictCard.svelte";
  import SyncConflictChoice from "$lib/components/sync/SyncConflictChoice.svelte";
  import { untrack } from "svelte";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import { notesBlockEditConflictView, previewNotesConflictText } from "$lib/notes/conflicts/presentation";
  import type { NotesBlockConflictChoice, NotesBlockEditConflict } from "$lib/stores/notes/persistence";

  let {
    conflicts,
    refusals = 0,
    onResolve,
  }: {
    conflicts: readonly NotesBlockEditConflict[];
    /** Grows each time leaving the page was refused because of these edits; each change calls for a choice. */
    refusals?: number;
    /** Applies a choice; rejects when it could not be saved. */
    onResolve: (blockId: string, choice: NotesBlockConflictChoice) => Promise<void>;
  } = $props();

  const { t } = getLocalization();
  const views = $derived(conflicts.map(notesBlockEditConflictView));
  let busy = $state<string | null>(null);
  let failed = $state<string | null>(null);
  let container = $state<HTMLDivElement | null>(null);
  // Refusals counted while no edit is held belong to edits already resolved.
  let settledRefusals = $state(untrack(() => refusals));
  const refused = $derived(conflicts.length > 0 && refusals > settledRefusals);

  $effect(() => {
    if (conflicts.length === 0) settledRefusals = refusals;
  });

  $effect(() => {
    // Reading the count scrolls again on every refusal, not only the first.
    if (refusals > 0 && refused) untrack(() => container?.scrollIntoView({ block: "nearest" }));
  });

  function cardMessage(blockId: string): string {
    if (failed === blockId) return t("notes.conflict.failed");
    return refused ? t("notes.conflict.chooseFirst") : "";
  }

  function choiceLabel(choice: NotesBlockConflictChoice, removed: boolean): string {
    if (choice === "both") return t("notes.conflict.keepBoth");
    if (removed) return choice === "mine" ? t("notes.conflict.restoreMine") : t("notes.conflict.discardMine");
    return choice === "mine" ? t("notes.conflict.useMine") : t("notes.conflict.useTheirs");
  }

  async function resolve(blockId: string, choice: NotesBlockConflictChoice): Promise<void> {
    if (busy) return;
    busy = blockId;
    failed = null;
    try {
      await onResolve(blockId, choice);
    } catch (cause: unknown) {
      console.warn("Notes block conflict could not be resolved", cause);
      failed = blockId;
    } finally {
      busy = null;
    }
  }
</script>

<div bind:this={container} class="flex flex-col gap-2 empty:hidden">
{#each views as view (view.blockId)}
  <SyncConflictCard
    title={view.removed ? t("notes.conflict.editRemovedTitle") : t("notes.conflict.editTitle")}
    hint={view.removed ? t("notes.conflict.editHint") : `${t("notes.conflict.editHint")} ${t("notes.conflict.keepBothHint")}`}
    message={cardMessage(view.blockId)}
  >
    {#each [{ label: t("notes.conflict.mine"), text: view.mine }, ...(view.theirs === null ? [] : [{ label: t("notes.conflict.theirs"), text: view.theirs }])] as version (version.label)}
      <div class="flex min-w-0 flex-col gap-1 rounded-md bg-black/5 px-2 py-1.5 dark:bg-white/5">
        <div class="text-muted-foreground">{version.label}</div>
        <div class="max-h-32 overflow-y-auto whitespace-pre-wrap wrap-anywhere">
          {#if version.text.trim()}
            {previewNotesConflictText(version.text)}
          {:else}
            <span class="text-muted-foreground">{t("notes.conflict.empty")}</span>
          {/if}
        </div>
      </div>
    {/each}
    <div class="flex flex-wrap items-center gap-1">
      {#each view.choices as choice (choice)}
        <SyncConflictChoice
          disabled={busy !== null}
          pending={busy === view.blockId}
          onclick={() => void resolve(view.blockId, choice)}
        >{choiceLabel(choice, view.removed)}</SyncConflictChoice>
      {/each}
    </div>
  </SyncConflictCard>
{/each}
</div>
