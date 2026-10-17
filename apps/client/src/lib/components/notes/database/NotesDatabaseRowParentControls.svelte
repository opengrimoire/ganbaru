<script lang="ts">
  import Select from "$lib/components/ui/Select.svelte";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import { untrack } from "svelte";
  import type { NotesPage } from "$lib/notes/types";
  import type { NotesDatabaseRowHierarchy } from "$lib/notes/database/row-hierarchy";

  let { rowId, rows, hierarchy, titleFor, pending = false, onSave }: {
    rowId: string;
    rows: NotesPage[];
    hierarchy: NotesDatabaseRowHierarchy;
    titleFor: (row: NotesPage) => string;
    pending?: boolean;
    onSave: (parentRowId: string | null) => Promise<void>;
  } = $props();
  const { t } = getLocalization();
  let parentId = $state(untrack(() => hierarchy[rowId]?.parent_row_page_id ?? ""));
  const savedParentId = $derived(hierarchy[rowId]?.parent_row_page_id ?? "");
  $effect(() => { parentId = savedParentId; });
  const candidates = $derived(rows.filter((row) => row.id !== rowId
    && !hierarchy[row.id]?.ancestor_row_page_ids.includes(rowId)));
  const unloadedParentId = $derived(savedParentId && !candidates.some((row) => row.id === savedParentId) ? savedParentId : null);
</script>

<div class="grid gap-2">
  <Select textSize="collection" inline appearance="quiet" class="w-full" ariaLabel={t("notes.databaseSubitemParent")}
    value={parentId} disabled={pending}
    options={[{ value: "", label: t("notes.databaseSubitemRoot") }, ...(unloadedParentId ? [{ value: unloadedParentId, label: t("notes.databaseSubitemUnloadedParent") }] : []), ...candidates.map((row) => ({ value: row.id, label: titleFor(row) }))]}
    onChange={(value) => { parentId = value; }} />
  <button type="button" class="min-h-8 rounded px-2 text-left hover:bg-accent" disabled={pending || parentId === (hierarchy[rowId]?.parent_row_page_id ?? "")}
    onclick={() => { void onSave(parentId || null); }}>{t("notes.databaseSubitemMoveApply")}</button>
</div>
