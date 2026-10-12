<script lang="ts">
  import Check from "@lucide/svelte/icons/check";
  import Minus from "@lucide/svelte/icons/minus";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import { notesDatabasePropertyDisplayText } from "$lib/notes/database/property-display";
  import { notesDatabaseTableCellEditValue, notesDatabaseTableOptionNames, type NotesDatabaseTableColumn } from "$lib/notes/database/table";
  import type { NotesPage } from "$lib/notes/types";
  import NotesDatabaseOptionBadge from "./NotesDatabaseOptionBadge.svelte";
  let { row, column }: { row: NotesPage; column: NotesDatabaseTableColumn } = $props();
  const localization = getLocalization();
  const { t } = localization;
  const text = $derived(notesDatabasePropertyDisplayText(row, column, localization));
  const value = $derived(notesDatabaseTableCellEditValue(row, column));
  const options = $derived(notesDatabaseTableOptionNames(row, column));
</script>

{#if column.type === "select" || column.type === "status" || column.type === "multi_select"}
  <span class="flex min-w-0 flex-wrap gap-1">{#each options as name}<NotesDatabaseOptionBadge label={name} color={column.options.find((option) => option.name === name)?.color} />{/each}</span>
{:else if column.type === "checkbox"}
  <span class="flex size-5 items-center justify-center rounded border border-border text-muted-foreground" aria-label={value === true ? t("notes.databaseTableFilterCondition.checked") : t("notes.databaseTableFilterCondition.unchecked")}>{#if value === true}<Check class="size-3.5" />{:else}<Minus class="size-3.5" />{/if}</span>
{:else}
  <span class={column.displayFormat?.wrap ? "whitespace-pre-wrap wrap-break-word" : "truncate"} title={text}>{text || t("notes.databaseTableEmptyCell")}</span>
{/if}
