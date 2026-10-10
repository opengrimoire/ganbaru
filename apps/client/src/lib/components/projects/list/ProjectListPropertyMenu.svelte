<script lang="ts">
  import { untrack } from "svelte";
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
  import Hash from "@lucide/svelte/icons/hash";
  import Pin from "@lucide/svelte/icons/pin";
  import PinOff from "@lucide/svelte/icons/pin-off";
  import Plus from "@lucide/svelte/icons/plus";
  import Settings2 from "@lucide/svelte/icons/settings-2";
  import Sigma from "@lucide/svelte/icons/sigma";
  import TextWrap from "@lucide/svelte/icons/text-wrap";
  import CollectionMenu from "$lib/components/collections/CollectionMenu.svelte";
  import CollectionMenuItem from "$lib/components/collections/CollectionMenuItem.svelte";
  import CollectionMenuSeparator from "$lib/components/collections/CollectionMenuSeparator.svelte";
  import CollectionPropertyNameField from "$lib/components/collections/CollectionPropertyNameField.svelte";
  import CollectionPropertyIcon from "$lib/components/collections/CollectionPropertyIcon.svelte";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import { getPreferences } from "$lib/stores/preferences.svelte";
  import { getProjects } from "$lib/stores/projects.svelte";
  import { projectCustomFieldUsesOptions, uniqueProjectCustomFieldName } from "$lib/projects/custom-fields";
  import { customFieldIdFromTaskListColumn } from "$lib/projects/tasks/list-columns";
  import { projectColumnCalculationOptions, projectListColumnSortMode, PROJECT_COLUMN_DATE_FORMATS, PROJECT_COLUMN_TIME_FORMATS, PROJECT_COLUMN_NUMBER_FORMATS } from "$lib/projects/list/presentation";
  import type { ProjectTaskListResizableColumn } from "$lib/projects/list/view";
  import { getProjectListTableContext } from "./table-context";
  import ProjectListPropertyDraft from "./ProjectListPropertyDraft.svelte";
  import ProjectListColumnFilters from "./ProjectListColumnFilters.svelte";
  import { projectListColumnKind } from "./property-kinds";

  /** Column header menu for the Projects task table, laid out like the Notes table column menu. */
  let { column, label }: { column: ProjectTaskListResizableColumn; label: string } = $props();
  const context = getProjectListTableContext();
  const query = context?.query;
  const projects = getProjects();
  const preferences = getPreferences();
  const localization = getLocalization();
  const { t } = localization;
  const field = $derived(column === "name" ? undefined : query?.customFields.find((candidate) => candidate.id === customFieldIdFromTaskListColumn(column)));
  const kind = $derived(projectListColumnKind(column, field));
  const pending = $derived(Boolean(query?.propertySaving || query?.presentationSaving || query?.listColumnsSaving));
  const sortMode = $derived(projectListColumnSortMode(column));
  const wrapped = $derived(query?.listPresentation.wrappedColumns.includes(column) ?? false);
  const frozen = $derived(query?.listPresentation.frozenThrough === column);
  const index = $derived(column === "name" ? -1 : query?.listColumns.indexOf(column) ?? -1);
  let optionDraft = $state("");
  let optionFieldId = $state(untrack(() => field?.id ?? null));
  let optionSaving = $state(false);
  let optionError = $state<string | null>(null);
  let optionWriteGeneration = 0;
  let trigger: HTMLButtonElement | undefined = $state();
  let insertSide = $state<"left" | "right" | null>(null);
  const insertAnchor = $derived(insertSide ? trigger?.closest<HTMLElement>("[role=\"columnheader\"]") ?? null : null);

  $effect(() => {
    const currentId = field?.id ?? null;
    untrack(() => {
      if (currentId === optionFieldId) return;
      optionWriteGeneration += 1;
      optionFieldId = currentId;
      optionDraft = "";
      optionSaving = false;
      optionError = null;
    });
  });

  /** Toggle text wrapping for this column in the saved list presentation. */
  function toggleWrap(): void {
    if (!query) return;
    void query.savePresentation({ ...query.listPresentation, wrappedColumns: wrapped
      ? query.listPresentation.wrappedColumns.filter((entry) => entry !== column)
      : [...query.listPresentation.wrappedColumns, column] });
  }

  /** Change the canonical query so sorting stays shared with toolbar controls and saved views. */
  function sort(direction: "asc" | "desc"): void {
    if (!query || !sortMode) return;
    query.sortMode = sortMode;
    query.sortDirection = direction;
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

  /** Add an option without modifying any existing task value. */
  async function addOption(): Promise<void> {
    if (!field || optionSaving || !optionDraft.trim()) return;
    const target = field;
    const name = optionDraft;
    const generation = ++optionWriteGeneration;
    optionSaving = true;
    optionError = null;
    try {
      if (projects.customFieldOptionsForField(target.id).some((candidate) => candidate.name.toLocaleLowerCase() === name.trim().toLocaleLowerCase())) throw new Error(t("projects.customFields.optionNameExists"));
      await projects.addCustomFieldOption(target.id, name);
      if (generation === optionWriteGeneration) optionDraft = "";
    } catch (error: unknown) {
      if (generation === optionWriteGeneration) optionError = t("projects.columns.propertyFailed", error instanceof Error ? error.message : String(error));
    } finally { if (generation === optionWriteGeneration) optionSaving = false; }
  }

  /** Use a unique copy name while keeping the source property's values independent. */
  function duplicate(): void {
    if (!query || !field) return;
    void query.addColumnProperty(uniqueProjectCustomFieldName(t("projects.columns.copyName", field.name), query.customFields, localization.locale), field.fieldType, column, "right", { source: field });
  }
</script>

{#if query}
  <CollectionMenu {label} kind="property" fullWidth showHeader={false} dismissOnAction disabled={pending}
    triggerClass="h-auto justify-start rounded-none px-2 font-normal" triggerAttributes={{ "data-collection-cell-primary": "" }} bind:triggerElement={trigger}>
    {#snippet leading()}{#if preferences.projectsShowPropertyIcons}<CollectionPropertyIcon icon={field?.icon ?? null} {kind} class="size-3.5 shrink-0 text-muted-foreground" />{/if}{/snippet}
    <div class="grid gap-0">
      <CollectionPropertyNameField propertyId={column} name={field?.name ?? label} icon={field?.icon ?? null} {kind} editable={Boolean(field)} onRename={rename} onIconChange={field ? changeIcon : undefined} />
      {#if field && projectCustomFieldUsesOptions(field.fieldType)}
        <CollectionMenu label={t("collections.property.editProperty")} kind="properties" icon={Settings2} fullWidth>
          <div class="grid gap-1">
            {#each projects.customFieldOptionsForField(field.id) as option (option.id)}<span class="truncate px-2 py-1">{option.name}</span>{/each}
            <form class="flex min-w-0 gap-1" onsubmit={(event) => { event.preventDefault(); void addOption(); }}>
              <input class="field min-w-0 flex-1" aria-label={t("projects.customFields.optionName")} placeholder={t("projects.customFields.optionName")} bind:value={optionDraft} disabled={optionSaving} />
              <button type="submit" class="collection-menu-control inline-flex items-center justify-center rounded-md hover:bg-accent" aria-label={t("projects.columns.addOption")} disabled={optionSaving || !optionDraft.trim()}><Plus class="size-3.5" aria-hidden="true" /></button>
            </form>
            {#if optionError}<p role="alert" class="px-1 py-1 text-destructive">{optionError}</p>{/if}
          </div>
        </CollectionMenu>
      {/if}
      <CollectionMenuSeparator />
      <ProjectListColumnFilters {query} {column} />
      {#if sortMode}
        <CollectionMenuItem icon={ArrowUp} label={t("collections.property.sortAscending")} checked={query.sortMode === sortMode && query.sortDirection === "asc"} onclick={() => sort("asc")} />
        <CollectionMenuItem icon={ArrowDown} label={t("collections.property.sortDescending")} checked={query.sortMode === sortMode && query.sortDirection === "desc"} onclick={() => sort("desc")} />
      {/if}
      <CollectionMenu label={t("collections.property.calculate")} kind="properties" icon={Sigma} fullWidth
        summary={(query.listPresentation.calculations[column] ?? "none") === "none" ? undefined : t("projects.columns.calculation", query.listPresentation.calculations[column] ?? "none")}>
        {#each projectColumnCalculationOptions(column, query.customFields) as calculation}
          <CollectionMenuItem label={t("projects.columns.calculation", calculation)} checked={(query.listPresentation.calculations[column] ?? "none") === calculation} disabled={pending}
            onclick={() => void query.savePresentation({ ...query.listPresentation, calculations: { ...query.listPresentation.calculations, [column]: calculation } })} />
        {/each}
      </CollectionMenu>
      {#if column === "start" || column === "due" || field?.fieldType === "date"}
        <CollectionMenu label={t("projects.columns.dateFormat")} kind="properties" icon={CalendarCog} fullWidth>
          {#each PROJECT_COLUMN_DATE_FORMATS as format}
            <CollectionMenuItem label={t("projects.columns.dateFormatLabel", format)} checked={(query.listPresentation.dateFormats[column] ?? "locale") === format} disabled={pending}
              onclick={() => void query.savePresentation({ ...query.listPresentation, dateFormats: { ...query.listPresentation.dateFormats, [column]: format } })} />
          {/each}
        </CollectionMenu>
      {/if}
      {#if column === "start" || column === "due"}
        <CollectionMenu label={t("projects.columns.timeFormat")} kind="properties" icon={Clock} fullWidth>
          {#each PROJECT_COLUMN_TIME_FORMATS as format}
            <CollectionMenuItem label={t("projects.columns.timeFormatLabel", format)} checked={(query.listPresentation.timeFormats[column] ?? "locale") === format} disabled={pending}
              onclick={() => void query.savePresentation({ ...query.listPresentation, timeFormats: { ...query.listPresentation.timeFormats, [column]: format } })} />
          {/each}
        </CollectionMenu>
      {/if}
      {#if field?.fieldType === "number"}
        <CollectionMenu label={t("projects.columns.numberFormat")} kind="properties" icon={Hash} fullWidth>
          {#each PROJECT_COLUMN_NUMBER_FORMATS as format}
            <CollectionMenuItem label={t("projects.columns.numberFormatLabel", format)} checked={(query.listPresentation.numberFormats[column] ?? "number") === format} disabled={pending}
              onclick={() => void query.savePresentation({ ...query.listPresentation, numberFormats: { ...query.listPresentation.numberFormats, [column]: format } })} />
          {/each}
        </CollectionMenu>
      {/if}
      <CollectionMenuSeparator />
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
      {#if field}<CollectionMenuItem icon={Copy} label={t("collections.property.duplicate")} disabled={pending} onclick={duplicate} />{/if}
    </div>
  </CollectionMenu>
  {#if insertSide && insertAnchor}
    <ProjectListPropertyDraft {query} {column} side={insertSide} anchor={insertAnchor} placement={insertSide === "left" ? "before" : "after"} {trigger}
      onClose={() => { insertSide = null; }} />
  {/if}
{:else}
  <span class="truncate">{label}</span>
{/if}
