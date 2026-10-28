<script lang="ts">
  import {
    NotesConflictError,
    type NotesConflictChoice,
    type NotesConflictVersion,
    type NotesFieldConflict,
  } from "$lib/api/notes/conflicts";
  import SyncConflictCard from "$lib/components/sync/SyncConflictCard.svelte";
  import SyncConflictChoice from "$lib/components/sync/SyncConflictChoice.svelte";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import {
    notesFieldConflictTitleKey,
    notesPropertyNameById,
    notesThreadStatusIsResolved,
    orderNotesFieldConflicts,
    previewNotesConflictText,
  } from "$lib/notes/conflicts/presentation";
  import { notesConflicts } from "$lib/stores/notes-conflicts.svelte";
  import { formatSyncTime, syncDeviceName } from "$lib/sync/status";

  /** Surfaced conflicts of a page; the parent reads them through the Notes conflicts store. */
  let {
    pageId,
    properties,
    blockOrder,
    onResolved,
  }: {
    pageId: string;
    /** The page's composed properties, used to name a property whose value is in conflict. */
    properties: Readonly<Record<string, unknown>>;
    /** Block ids in page order, so block conflicts follow the page. */
    blockOrder: readonly string[];
    /** Runs after a resolution so the open page shows the kept version. */
    onResolved: () => Promise<void> | void;
  } = $props();

  const localization = getLocalization();
  const { t } = localization;
  const conflicts = $derived(orderNotesFieldConflicts(notesConflicts.read(pageId)?.conflicts ?? [], blockOrder));
  let busy = $state(false);
  let outcome = $state<{ key: string; kind: "resolved" | "error"; message: string } | null>(null);

  function conflictKey(conflict: NotesFieldConflict): string {
    return `${conflict.target}:${conflict.id}`;
  }

  function title(conflict: NotesFieldConflict): string {
    const key = notesFieldConflictTitleKey(conflict.target);
    if (key) return t(key);
    return t("notes.conflict.valueTitle", notesPropertyNameById(properties, conflict.id) ?? t("notes.conflict.unnamedProperty"));
  }

  function versionText(conflict: NotesFieldConflict, version: NotesConflictVersion): string {
    if (conflict.target !== "thread_status") return version.text;
    return notesThreadStatusIsResolved(version.text) ? t("notes.conflict.threadResolved") : t("notes.conflict.threadOpen");
  }

  function source(version: NotesConflictVersion): string {
    return t(
      "notes.conflict.versionFrom",
      syncDeviceName(version.device, t("notes.conflict.thisDevice"), t("notes.conflict.otherDevice")),
      formatSyncTime(t, localization.locale, version.editedAtMs, Date.now()),
    );
  }

  async function resolve(conflict: NotesFieldConflict, choice: NotesConflictChoice): Promise<void> {
    if (busy) return;
    busy = true;
    outcome = null;
    const key = conflictKey(conflict);
    try {
      await notesConflicts.resolve({ pageId, target: conflict.target, id: conflict.id, choice });
      await onResolved();
    } catch (cause: unknown) {
      if (cause instanceof NotesConflictError && cause.code === "resolved") {
        outcome = { key, kind: "resolved", message: t("notes.conflict.resolved") };
      } else if (cause instanceof NotesConflictError && cause.code === "unfit") {
        outcome = { key, kind: "error", message: t("notes.conflict.unfit") };
      } else {
        console.warn("Notes conflict could not be resolved", cause);
        outcome = { key, kind: "error", message: t("notes.conflict.failed") };
      }
    } finally {
      busy = false;
    }
  }
</script>

{#each conflicts as conflict (conflictKey(conflict))}
  {@const key = conflictKey(conflict)}
  <SyncConflictCard
    title={title(conflict)}
    hint={t("notes.conflict.hint")}
    message={outcome?.key === key ? outcome.message : ""}
    messageKind={outcome?.key === key ? outcome.kind : "error"}
  >
    {#each conflict.versions as version (version.version)}
      {@const text = versionText(conflict, version)}
      <div class="flex min-w-0 flex-col gap-1 rounded-md bg-black/5 px-2 py-1.5 dark:bg-white/5">
        <div class="max-h-32 overflow-y-auto whitespace-pre-wrap wrap-anywhere">
          {#if text.trim()}
            {previewNotesConflictText(text)}
          {:else}
            <span class="text-muted-foreground">{t("notes.conflict.empty")}</span>
          {/if}
        </div>
        <div class="flex flex-wrap items-baseline gap-x-2 text-muted-foreground">
          <span>{source(version)}</span>
          {#if version.displayed}<span>{t("notes.conflict.shown")}</span>{/if}
        </div>
        <div class="flex flex-wrap items-center gap-1">
          <SyncConflictChoice
            disabled={busy}
            onclick={() => void resolve(conflict, version.displayed ? { kind: "displayed" } : { kind: "version", version: version.version })}
          >{version.displayed ? t("notes.conflict.keepThis") : t("notes.conflict.useOther")}</SyncConflictChoice>
        </div>
      </div>
    {/each}
  </SyncConflictCard>
{/each}
