<script lang="ts">
  import {
    getQuickNoteConflict,
    QuickNoteConflictError,
    resolveQuickNoteConflict,
  } from "$lib/api/quick-notes";
  import Info from "@lucide/svelte/icons/info";
  import Notice from "$lib/components/ui/Notice.svelte";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import { quickNotePlainText } from "$lib/quick-notes/rich-text";
  import type {
    QuickNoteConflict,
    QuickNoteConflictChoice,
    QuickNoteConflictField,
    QuickNoteConflictResolved,
    QuickNoteConflictVersion,
  } from "$lib/quick-notes/types";
  import { formatSyncTime, syncDeviceName } from "$lib/sync/status";

  let {
    noteId,
    revision,
    onResolved,
    onStale,
    mobileLayout = false,
  }: {
    noteId: string;
    /** Canonical revision the editor shows; a change reloads the versions. */
    revision: number;
    onResolved: (result: QuickNoteConflictResolved) => void;
    /** Called when another device or window already resolved the conflict. */
    onStale: () => void;
    mobileLayout?: boolean;
  } = $props();

  const PREVIEW_MAX_CHARS = 140;
  const localization = getLocalization();
  const { t } = localization;
  let conflict = $state<QuickNoteConflict | null>(null);
  let loading = $state(true);
  let busy = $state(false);
  let message = $state("");
  let messageKind = $state<"resolved" | "loadFailed" | "resolveFailed">("resolveFailed");
  let loadGeneration = 0;

  const actionClass = $derived(mobileLayout
    ? "min-h-12 rounded-xl px-3 font-medium underline active:bg-black/10 disabled:opacity-40 dark:active:bg-white/10"
    : "rounded-md px-1.5 py-0.5 font-medium underline hover:bg-black/10 disabled:opacity-40 dark:hover:bg-white/10");

  function preview(version: QuickNoteConflictVersion): string {
    const text = version.title ?? quickNotePlainText(version.runs ?? []);
    const trimmed = text.trim();
    if (!trimmed) return t("quickNotes.conflict.empty");
    return trimmed.length > PREVIEW_MAX_CHARS ? `${trimmed.slice(0, PREVIEW_MAX_CHARS)}...` : trimmed;
  }

  function fieldLabel(field: QuickNoteConflictField): string {
    return field === "title" ? t("quickNotes.conflict.titleField") : t("quickNotes.conflict.bodyField");
  }

  function source(version: QuickNoteConflictVersion): string {
    return t(
      "quickNotes.conflict.versionFrom",
      syncDeviceName(version.device, t("quickNotes.conflict.thisDevice"), t("quickNotes.conflict.otherDevice")),
      formatSyncTime(t, localization.locale, version.editedAtMs, Date.now()),
    );
  }

  async function load(id: string): Promise<void> {
    const expected = ++loadGeneration;
    loading = true;
    try {
      const next = await getQuickNoteConflict(id);
      if (expected !== loadGeneration) return;
      conflict = next;
      message = "";
      if (next.groups.length === 0) onStale();
    } catch (cause: unknown) {
      if (expected !== loadGeneration) return;
      if (cause instanceof QuickNoteConflictError && cause.code === "resolved") {
        onStale();
        return;
      }
      console.warn("quick note conflict could not be loaded", cause);
      message = t("quickNotes.conflict.loadFailed");
      messageKind = "loadFailed";
    } finally {
      if (expected === loadGeneration) loading = false;
    }
  }

  async function resolve(field: QuickNoteConflictField, choice: QuickNoteConflictChoice): Promise<void> {
    if (busy) return;
    busy = true;
    message = "";
    try {
      onResolved(await resolveQuickNoteConflict({ id: noteId, field, choice }));
    } catch (cause: unknown) {
      if (cause instanceof QuickNoteConflictError && cause.code === "resolved") {
        message = t("quickNotes.conflict.resolved");
        messageKind = "resolved";
        onStale();
      } else {
        console.warn("quick note conflict could not be resolved", cause);
        message = t("quickNotes.conflict.failed");
        messageKind = "resolveFailed";
      }
    } finally {
      busy = false;
    }
  }

  $effect(() => {
    void revision;
    void load(noteId);
  });
</script>

<div class="mb-2 flex max-h-56 flex-col gap-2 overflow-y-auto rounded-md bg-warning/15 px-2.5 py-2 text-xs" role="region" aria-label={t("quickNotes.conflict.title")}>
  <div class="font-medium">{t("quickNotes.conflict.title")}</div>
  {#if loading && !conflict}
    <div class="opacity-70" role="status">{t("quickNotes.conflict.loading")}</div>
  {/if}
  {#each conflict?.groups ?? [] as group (group.field)}
    <div class="flex flex-col gap-1.5">
      <div class="font-medium opacity-80">{fieldLabel(group.field)}</div>
      {#each group.versions as version (version.version)}
        <div class="flex flex-col gap-0.5 rounded-md bg-black/5 px-2 py-1.5 dark:bg-white/5">
          <div class="flex flex-wrap items-baseline gap-x-2 opacity-70">
            <span>{source(version)}</span>
            {#if version.displayed}<span>{t("quickNotes.conflict.shown")}</span>{/if}
          </div>
          <div class="line-clamp-3 whitespace-pre-wrap wrap-break-word">{preview(version)}</div>
          <div class="flex flex-wrap items-center gap-1">
            {#if version.displayed}
              <button class={actionClass} type="button" disabled={busy} onclick={() => void resolve(group.field, { kind: "displayed" })}>{t("quickNotes.conflict.keepThis")}</button>
            {:else}
              <button class={actionClass} type="button" disabled={busy} onclick={() => void resolve(group.field, { kind: "version", version: version.version })}>{t("quickNotes.conflict.useOther")}</button>
              <button class={actionClass} type="button" disabled={busy} title={t("quickNotes.conflict.keepBothHint")} onclick={() => void resolve(group.field, { kind: "keep_both", version: version.version })}>{t("quickNotes.conflict.keepBoth")}</button>
            {/if}
          </div>
        </div>
      {/each}
    </div>
  {/each}
  {#if message}
    <div data-quick-note-notice>
      <Notice
        {message}
        icon={messageKind === "resolved" ? Info : undefined}
        role={messageKind === "resolved" ? "status" : "alert"}
        copy={false}
        onRetry={messageKind === "loadFailed" ? () => load(noteId) : undefined}
      />
    </div>
  {/if}
</div>
