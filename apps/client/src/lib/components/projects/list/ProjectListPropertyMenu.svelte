<script lang="ts">
  import ArrowDown from "@lucide/svelte/icons/arrow-down";
  import ArrowLeft from "@lucide/svelte/icons/arrow-left";
  import ArrowLeftToLine from "@lucide/svelte/icons/arrow-left-to-line";
  import ArrowRight from "@lucide/svelte/icons/arrow-right";
  import ArrowRightToLine from "@lucide/svelte/icons/arrow-right-to-line";
  import ArrowUp from "@lucide/svelte/icons/arrow-up";
  import CalendarCog from "@lucide/svelte/icons/calendar-cog";
  import Clock from "@lucide/svelte/icons/clock";
  import Copy from "@lucide/svelte/icons/copy";
  import EyeOff from "@lucide/svelte/icons/eye-off";
  import Group from "@lucide/svelte/icons/group";
  import Hash from "@lucide/svelte/icons/hash";
  import Pin from "@lucide/svelte/icons/pin";
  import PinOff from "@lucide/svelte/icons/pin-off";
  import Settings2 from "@lucide/svelte/icons/settings-2";
  import Sigma from "@lucide/svelte/icons/sigma";
  import TextWrap from "@lucide/svelte/icons/text-wrap";
  import Trash2 from "@lucide/svelte/icons/trash-2";
  import CollectionMenu from "$lib/components/collections/CollectionMenu.svelte";
  import CollectionMenuItem from "$lib/components/collections/CollectionMenuItem.svelte";
  import CollectionMenuSelect from "$lib/components/collections/CollectionMenuSelect.svelte";
  import CollectionMenuSeparator from "$lib/components/collections/CollectionMenuSeparator.svelte";
  import CollectionPropertyNameField from "$lib/components/collections/CollectionPropertyNameField.svelte";
  import CollectionPropertyIcon from "$lib/components/collections/CollectionPropertyIcon.svelte";
  import ConfirmDialog from "$lib/components/ui/ConfirmDialog.svelte";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import { getPreferences } from "$lib/stores/preferences.svelte";
  import { useProjects } from "$lib/stores/projects.svelte";
  import { projectCustomFieldUsesOptions, uniqueProjectCustomFieldName } from "$lib/projects/custom-fields";
  import { customFieldIdFromTaskListColumn } from "$lib/projects/tasks/list-columns";
  import {
    projectColumnCalculationOptions,
    projectListColumnGroupMode,
    projectListColumnSortMode,
    PROJECT_COLUMN_DATE_FORMATS,
    PROJECT_COLUMN_NUMBER_FORMATS,
    PROJECT_COLUMN_TIME_FORMATS,
  } from "$lib/projects/list/presentation";
  import type { ProjectTaskListResizableColumn } from "$lib/projects/list/view";
  import type { ProjectCustomField } from "$lib/projects/types";
  import { getProjectListTableContext } from "./table-context";
  import ProjectListPropertyDraft from "./ProjectListPropertyDraft.svelte";
  import ProjectListColumnFilters from "./ProjectListColumnFilters.svelte";
  import ProjectListPropertyOptionsEditor from "./ProjectListPropertyOptionsEditor.svelte";
  import { projectListColumnKind } from "./property-kinds";

  /** Column header menu for the Projects task table, laid out like the Notes table column menu. */
  let { column, label }: { column: ProjectTaskListResizableColumn; label: string } = $props();
  const context = getProjectListTableContext();
  const query = context?.query;
  const projects = useProjects();
  const preferences = getPreferences();
  const localization = getLocalization();
  const { t } = localization;
  const field = $derived(column === "name" ? undefined : query?.customFields.find((candidate) => candidate.id === customFieldIdFromTaskListColumn(column)));
  const kind = $derived(projectListColumnKind(column, field));
  const pending = $derived(Boolean(query?.propertySaving || query?.presentationSaving || query?.listColumnsSaving));
  const sortMode = $derived(projectListColumnSortMode(column));
  const groupMode = $derived(projectListColumnGroupMode(column));
  const optionField = $derived(field && projectCustomFieldUsesOptions(field.fieldType) ? field : undefined);
  const dateColumn = $derived(column === "start" || column === "due" || field?.fieldType === "date");
  /** Only the built-in start and due columns carry a time. */
  const timeColumn = $derived(column === "start" || column === "due" ? column : null);
  const numberColumn = $derived(field?.fieldType === "number");
  const wrapped = $derived(query?.listPresentation.wrappedColumns.includes(column) ?? false);
  const frozen = $derived(query?.listPresentation.frozenThrough === column);
  const index = $derived(column === "name" ? -1 : query?.listColumns.indexOf(column) ?? -1);
  const sortDirection = $derived(query && sortMode && query.sortMode === sortMode ? query.sortDirection : null);
  let trigger: HTMLButtonElement | undefined = $state();
  let insertSide = $state<"left" | "right" | null>(null);
  let pendingDelete = $state<ProjectCustomField | null>(null);
  const insertAnchor = $derived(insertSide ? trigger?.closest<HTMLElement>("[role=\"columnheader\"]") ?? null : null);

  /** Toggle text wrapping for this column in the saved list presentation. */
  function toggleWrap(): void {
    if (!query) return;
    void query.savePresentation({ ...query.listPresentation, wrappedColumns: wrapped
      ? query.listPresentation.wrappedColumns.filter((entry) => entry !== column)
      : [...query.listPresentation.wrappedColumns, column] });
  }

  /** Change the canonical query so sorting stays shared with toolbar controls. */
  function sort(direction: "asc" | "desc"): void {
    if (!query || !sortMode) return;
    query.sortMode = sortMode;
    query.sortDirection = direction;
  }

  /** Group tasks by this column, or return to sections when it already drives the grouping. */
  function toggleGrouping(): void {
    if (!query || !groupMode) return;
    query.groupBy = query.groupBy === groupMode ? "section" : groupMode;
  }

  /** Rename the custom field; rejections are shown by the name field, which keeps the draft. */
  async function rename(name: string): Promise<void> {
    if (!field) return;
    const target = field;
    if (query?.customFields.some((candidate) => candidate.id !== target.id && candidate.name.toLocaleLowerCase() === name.toLocaleLowerCase())) throw new Error(t("projects.columns.nameExists"));
    await projects.updateCustomField(target, { name });
  }

  /** Change the custom field icon; rejections are shown by the name field. */
  async function changeIcon(icon: string | null): Promise<void> {
    if (!field) return;
    await projects.updateCustomField(field, { icon });
  }

  /** Use a unique copy name while keeping the source property's values independent. */
  function duplicate(): void {
    if (!query || !field) return;
    void query.addColumnProperty(uniqueProjectCustomFieldName(t("projects.columns.copyName", field.name), query.customFields, localization.locale), field.fieldType, column, "right", { source: field });
  }

  /** Delete the confirmed field; failures appear with the table's other property errors. */
  function confirmDelete(): void {
    const target = pendingDelete;
    pendingDelete = null;
    if (target && query) void query.deleteColumnProperty(target);
  }
</script>

{#if query}
  <CollectionMenu {label} kind="property" fullWidth showHeader={false} dismissOnAction contextMenu disabled={query.propertySaving}
    triggerClass="h-auto justify-start rounded-none px-2 font-normal" triggerAttributes={{ "data-collection-cell-primary": "" }} bind:triggerElement={trigger}>
    {#snippet leading()}{#if preferences.projectsShowPropertyIcons}<CollectionPropertyIcon icon={field?.icon ?? null} {kind} class="size-3.5 shrink-0 text-muted-foreground" />{/if}{/snippet}
    <div class="grid gap-0">
      <CollectionPropertyNameField propertyId={column} name={field?.name ?? label} icon={field?.icon ?? null} {kind} editable={Boolean(field)} onRename={rename} onIconChange={field ? changeIcon : undefined} />
      {#if optionField || dateColumn || numberColumn}
        <CollectionMenu label={t("collections.property.editProperty")} kind="properties" icon={Settings2} fullWidth disabled={pending}>
          {#if optionField}
            {#key optionField.id}<ProjectListPropertyOptionsEditor field={optionField} disabled={pending} />{/key}
          {/if}
          {#if dateColumn}
            <CollectionMenuSelect label={t("projects.columns.dateFormat")} icon={CalendarCog} value={query.listPresentation.dateFormats[column] ?? "locale"} disabled={pending}
              options={PROJECT_COLUMN_DATE_FORMATS.map((format) => ({ value: format, label: t("projects.columns.dateFormatLabel", format) }))}
              onChange={(format) => void query.savePresentation({ ...query.listPresentation, dateFormats: { ...query.listPresentation.dateFormats, [column]: format } })} />
          {/if}
          {#if timeColumn}
            <CollectionMenuSelect label={t("projects.columns.timeFormat")} icon={Clock} value={query.listPresentation.timeFormats[timeColumn] ?? "locale"} disabled={pending}
              options={PROJECT_COLUMN_TIME_FORMATS.map((format) => ({ value: format, label: t("projects.columns.timeFormatLabel", format) }))}
              onChange={(format) => void query.savePresentation({ ...query.listPresentation, timeFormats: { ...query.listPresentation.timeFormats, [timeColumn]: format } })} />
          {/if}
          {#if numberColumn}
            <CollectionMenuSelect label={t("projects.columns.numberFormat")} icon={Hash} value={query.listPresentation.numberFormats[column] ?? "number"} disabled={pending}
              options={PROJECT_COLUMN_NUMBER_FORMATS.map((format) => ({ value: format, label: t("projects.columns.numberFormatLabel", format) }))}
              onChange={(format) => void query.savePresentation({ ...query.listPresentation, numberFormats: { ...query.listPresentation.numberFormats, [column]: format } })} />
          {/if}
        </CollectionMenu>
      {/if}
      <CollectionMenuSeparator />
      <ProjectListColumnFilters {query} {column} />
      {#if sortMode}
        <CollectionMenu label={t("collections.property.sort")} kind="sort" fullWidth dismissOnAction
          summary={sortDirection ? t(sortDirection === "asc" ? "collections.property.sortAscending" : "collections.property.sortDescending") : undefined}>
          <CollectionMenuItem icon={ArrowUp} label={t("collections.property.sortAscending")} checked={sortDirection === "asc"} onclick={() => sort("asc")} />
          <CollectionMenuItem icon={ArrowDown} label={t("collections.property.sortDescending")} checked={sortDirection === "desc"} onclick={() => sort("desc")} />
        </CollectionMenu>
      {/if}
      {#if groupMode}
        <CollectionMenuItem icon={Group} label={t(query.groupBy === groupMode ? "collections.property.ungroup" : "collections.property.group")} onclick={toggleGrouping} />
      {/if}
      <CollectionMenu label={t("collections.property.calculate")} kind="properties" icon={Sigma} fullWidth
        summary={(query.listPresentation.calculations[column] ?? "none") === "none" ? undefined : t("projects.columns.calculation", query.listPresentation.calculations[column] ?? "none")}>
        {#each projectColumnCalculationOptions(column, query.customFields) as calculation}
          <CollectionMenuItem label={t("projects.columns.calculation", calculation)} checked={(query.listPresentation.calculations[column] ?? "none") === calculation} disabled={pending}
            onclick={() => void query.savePresentation({ ...query.listPresentation, calculations: { ...query.listPresentation.calculations, [column]: calculation } })} />
        {/each}
      </CollectionMenu>
      <CollectionMenuItem icon={frozen ? PinOff : Pin} label={t(frozen ? "collections.property.unfreeze" : "collections.property.freeze")} disabled={pending}
        onclick={() => void query.savePresentation({ ...query.listPresentation, frozenThrough: frozen ? null : column })} />
      {#if column !== "name"}<CollectionMenuItem icon={EyeOff} label={t("collections.property.hide")} disabled={pending} onclick={() => void query.toggleColumn(column)} />{/if}
      <CollectionMenuItem icon={TextWrap} label={t("collections.property.wrap")} checked={wrapped} disabled={pending} onclick={toggleWrap} />
      <CollectionMenuSeparator />
      {#if column !== "name"}
        <CollectionMenuItem icon={ArrowLeft} label={t("collections.property.moveLeft")} disabled={pending || index <= 0} onclick={() => void query.moveColumn(column, -1)} />
        <CollectionMenuItem icon={ArrowRight} label={t("collections.property.moveRight")} disabled={pending || index < 0 || index >= query.listColumns.length - 1} onclick={() => void query.moveColumn(column, 1)} />
        <CollectionMenuItem icon={ArrowLeftToLine} label={t("collections.property.insertLeft")} disabled={pending} onclick={() => { insertSide = "left"; }} />
      {/if}
      <CollectionMenuItem icon={ArrowRightToLine} label={t("collections.property.insertRight")} disabled={pending} onclick={() => { insertSide = "right"; }} />
      {#if field}
        <CollectionMenuItem icon={Copy} label={t("collections.property.duplicate")} disabled={pending} onclick={duplicate} />
        <CollectionMenuItem icon={Trash2} label={t("collections.property.delete")} destructive disabled={pending} onclick={() => { pendingDelete = field ?? null; }} />
      {/if}
    </div>
  </CollectionMenu>
  {#if insertSide && insertAnchor}
    <ProjectListPropertyDraft {query} {column} side={insertSide} anchor={insertAnchor} placement={insertSide === "left" ? "before" : "after"} {trigger}
      onClose={() => { insertSide = null; }} />
  {/if}
  {#if pendingDelete}
    <ConfirmDialog title={t("projects.customFields.deleteFieldTitle", pendingDelete.name)} message={t("projects.customFields.deleteFieldMessage", pendingDelete.name)}
      confirmLabel={t("projects.customFields.deleteFieldConfirm")} cancelLabel={t("common.cancel")}
      onConfirm={confirmDelete} onCancel={() => { pendingDelete = null; }} />
  {/if}
{:else}
  <span class="truncate">{label}</span>
{/if}
