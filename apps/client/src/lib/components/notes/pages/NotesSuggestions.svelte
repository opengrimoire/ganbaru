<script lang="ts">
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import { blockPlainText } from "$lib/notes/blocks/factory";
  import {
    notesResolveSuggestionAnchor,
    openNotesSuggestionCount,
  } from "$lib/notes/collaboration/suggestions";
  import type { NotesSuggestion } from "$lib/notes/types";
  import { getNotesEditor } from "$lib/components/notes/editor-context";
  import Check from "@lucide/svelte/icons/check";
  import ChevronDown from "@lucide/svelte/icons/chevron-down";
  import PencilLine from "@lucide/svelte/icons/pencil-line";
  import X from "@lucide/svelte/icons/x";
  import Checkbox from "$lib/components/ui/Checkbox.svelte";
  import Notice from "$lib/components/ui/Notice.svelte";

  const notes = getNotesEditor();
  const localization = getLocalization();
  const { t } = localization;
  let { embedded = false }: { embedded?: boolean } = $props();
  let open = $state(false);
  let proposedDraft = $state("");
  let lastDraftKey = $state("");
  let actionBusyId = $state<string | null>(null);
  const draft = $derived(notes.activeSuggestionDraft);
  const openSuggestionCount = $derived(openNotesSuggestionCount(notes.suggestions));
  const panelOpen = $derived(embedded || open);

  $effect(() => {
    if (!draft) return;
    open = true;
    const key = `${draft.block_id}:${draft.range_start}:${draft.range_end}:${draft.original_text}`;
    if (lastDraftKey === key) return;
    lastDraftKey = key;
    proposedDraft = draft.proposed_text;
  });

  function suggestionTime(suggestion: NotesSuggestion): string {
    return new Date(suggestion.last_edited_time).toLocaleString(localization.locale);
  }

  function displayName(suggestion: NotesSuggestion): string {
    return suggestion.display_name.resolved_name;
  }

  function suggestionStatusLabel(suggestion: NotesSuggestion): string {
    switch (suggestion.status) {
      case "open":
        return t("notes.suggestionStatusOpen");
      case "accepted":
        return t("notes.suggestionStatusAccepted");
      case "rejected":
        return t("notes.suggestionStatusRejected");
    }
  }

  function isSuggestionTargetMissing(suggestion: NotesSuggestion): boolean {
    const block = notes.blockById(suggestion.block_id);
    return !block || !notesResolveSuggestionAnchor(suggestion, blockPlainText(block));
  }

  function suggestionErrorMessage(message: string): string {
    return message === "target_missing"
      ? t("notes.suggestionTargetMissing")
      : t("notes.loadSuggestionsFailed", message);
  }

  async function saveSuggestionDraft(): Promise<void> {
    const currentDraft = draft;
    if (!currentDraft || proposedDraft === currentDraft.original_text) return;
    await notes.createSuggestion(proposedDraft);
    proposedDraft = "";
    lastDraftKey = "";
  }

  async function runSuggestionAction(
    suggestion: NotesSuggestion,
    action: "accept" | "reject",
  ): Promise<void> {
    if (actionBusyId) return;
    actionBusyId = suggestion.id;
    try {
      if (action === "accept") {
        await notes.acceptSuggestion(suggestion.id);
      } else {
        await notes.rejectSuggestion(suggestion.id);
      }
    } finally {
      actionBusyId = null;
    }
  }

  function cancelDraft(): void {
    notes.cancelSuggestionDraft();
    proposedDraft = "";
    lastDraftKey = "";
  }
</script>

<div class={embedded ? "min-w-0" : "mt-2"}>
  {#if !embedded}
    <button
      class="inline-flex items-center gap-1.5 rounded-md px-1.5 py-1 text-[0.733333rem] text-muted-foreground hover:bg-accent hover:text-foreground"
      type="button"
      aria-expanded={open}
      onclick={() => {
        open = !open;
      }}
    >
      <PencilLine class="size-3.5" />
      <span>
        {#if notes.suggestionsLoading}
          {t("notes.loadingSuggestions")}
        {:else}
          {t("notes.suggestionsCount", openSuggestionCount)}
        {/if}
      </span>
      <ChevronDown class={`size-3.5 transition-transform ${open ? "rotate-180" : ""}`} />
    </button>
  {/if}

  {#if panelOpen}
    <section class={embedded ? "rounded-md bg-muted/25 p-2" : "mt-2 max-w-3xl rounded-md border border-border bg-muted/25 p-2"}>
      <div class="flex flex-wrap items-center justify-between gap-2">
        <div class="text-[0.8rem] font-medium text-foreground">
          {t("notes.suggestedEdits")}
        </div>
        <label class="inline-flex cursor-pointer items-center gap-1.5 text-[0.733333rem] text-muted-foreground hover:text-foreground">
          <Checkbox
            checked={notes.suggestionsIncludeDecided}
            onChange={(checked) => {
              void notes.setSuggestionsIncludeDecided(checked);
            }}
          />
          <span>{t("notes.showDecidedSuggestions")}</span>
        </label>
      </div>

      {#if notes.suggestionsError}
        <Notice class="mt-2 text-[0.8rem]" message={suggestionErrorMessage(notes.suggestionsError)} />
      {/if}

      {#if draft}
        <div class="mt-2 rounded-md border border-primary/35 bg-primary/5 p-2">
          <div class="text-[0.733333rem] font-medium text-muted-foreground">
            {t("notes.newSuggestion")}
          </div>
          <div class="mt-1 grid gap-2 sm:grid-cols-2">
            <div class="min-w-0 rounded-md bg-background px-2 py-1.5">
              <div class="text-[0.7rem] text-muted-foreground">{t("notes.originalText")}</div>
              <div class="mt-1 whitespace-pre-wrap text-[0.866667rem] text-foreground">
                {draft.original_text}
              </div>
            </div>
            <label class="min-w-0">
              <span class="text-[0.7rem] text-muted-foreground">{t("notes.proposedText")}</span>
              <textarea
                class="field mt-1 min-h-20 w-full resize-y py-1.5 text-[0.866667rem]"
                bind:value={proposedDraft}
                aria-label={t("notes.proposedText")}
              ></textarea>
            </label>
          </div>
          <div class="mt-2 flex flex-wrap items-center gap-2">
            <button
              class="rounded-md bg-primary px-2.5 py-1.5 text-[0.8rem] font-medium text-primary-foreground disabled:opacity-50"
              type="button"
              disabled={proposedDraft === draft.original_text}
              onclick={() => {
                void saveSuggestionDraft();
              }}
            >
              {t("notes.createSuggestion")}
            </button>
            <button
              class="rounded-md px-2.5 py-1.5 text-[0.8rem] text-muted-foreground hover:bg-accent hover:text-foreground"
              type="button"
              onclick={cancelDraft}
            >
              {t("common.cancel")}
            </button>
          </div>
        </div>
      {/if}

      <div class="mt-3 flex min-w-0 flex-col gap-2">
        {#if notes.suggestionsLoading}
          <div class="text-[0.8rem] text-muted-foreground">{t("notes.loadingSuggestions")}</div>
        {:else if notes.suggestions.length === 0}
          <div class="text-[0.8rem] text-muted-foreground">{t("notes.noSuggestions")}</div>
        {:else}
          {#each notes.suggestions as suggestion (suggestion.id)}
            <article class="rounded-md border border-border bg-background p-2">
              <div class="flex flex-wrap items-start justify-between gap-2">
                <div class="min-w-0">
                  <div class="flex min-w-0 flex-wrap items-center gap-1.5">
                    <span class="truncate text-[0.8rem] font-medium text-foreground">
                      {displayName(suggestion)}
                    </span>
                    <span class="rounded bg-muted px-1.5 py-0.5 text-[0.7rem] text-muted-foreground">
                      {suggestionStatusLabel(suggestion)}
                    </span>
                  </div>
                  <div class="mt-0.5 text-[0.733333rem] text-muted-foreground">
                    {suggestionTime(suggestion)}
                  </div>
                  {#if isSuggestionTargetMissing(suggestion)}
                    <div class="mt-1 text-[0.733333rem] text-muted-foreground">
                      {t("notes.suggestionTargetMissing")}
                    </div>
                  {/if}
                </div>
                {#if suggestion.status === "open"}
                  <div class="flex shrink-0 items-center gap-1">
                    <button
                      class="inline-flex items-center gap-1 rounded-md px-2 py-1 text-[0.733333rem] text-muted-foreground hover:bg-accent hover:text-foreground disabled:opacity-50"
                      type="button"
                      disabled={actionBusyId !== null}
                      onclick={() => {
                        void runSuggestionAction(suggestion, "accept");
                      }}
                    >
                      <Check class="size-3.5" />
                      <span>{t("notes.acceptSuggestion")}</span>
                    </button>
                    <button
                      class="inline-flex items-center gap-1 rounded-md px-2 py-1 text-[0.733333rem] text-muted-foreground hover:bg-accent hover:text-foreground disabled:opacity-50"
                      type="button"
                      disabled={actionBusyId !== null}
                      onclick={() => {
                        void runSuggestionAction(suggestion, "reject");
                      }}
                    >
                      <X class="size-3.5" />
                      <span>{t("notes.rejectSuggestion")}</span>
                    </button>
                  </div>
                {/if}
              </div>
              <div class="mt-2 grid gap-2 sm:grid-cols-2">
                <div class="min-w-0 rounded-md bg-muted/40 px-2 py-1.5">
                  <div class="text-[0.7rem] text-muted-foreground">{t("notes.originalText")}</div>
                  <div class="mt-1 whitespace-pre-wrap text-[0.866667rem] text-foreground">
                    {suggestion.original_text}
                  </div>
                </div>
                <div class="min-w-0 rounded-md bg-muted/40 px-2 py-1.5">
                  <div class="text-[0.7rem] text-muted-foreground">{t("notes.proposedText")}</div>
                  <div class="mt-1 whitespace-pre-wrap text-[0.866667rem] text-foreground">
                    {suggestion.proposed_text}
                  </div>
                </div>
              </div>
            </article>
          {/each}
        {/if}
      </div>
    </section>
  {/if}
</div>
