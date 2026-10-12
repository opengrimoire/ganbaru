<script lang="ts">
  import { tick } from "svelte";
  import Check from "@lucide/svelte/icons/check";
  import CircleCheck from "@lucide/svelte/icons/circle-check";
  import GripVertical from "@lucide/svelte/icons/grip-vertical";
  import Plus from "@lucide/svelte/icons/plus";
  import Trash2 from "@lucide/svelte/icons/trash-2";
  import CollectionMenu from "$lib/components/collections/CollectionMenu.svelte";
  import CollectionMenuItem from "$lib/components/collections/CollectionMenuItem.svelte";
  import CollectionMenuSeparator from "$lib/components/collections/CollectionMenuSeparator.svelte";
  import CollectionMenuTextField from "$lib/components/collections/CollectionMenuTextField.svelte";
  import { getCollectionSettingsNavigation } from "$lib/components/collections/collection-settings-context";
  import Notice from "$lib/components/ui/Notice.svelte";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import { notesBlockColorStyle } from "$lib/notes/blocks/color";
  import type { NotesDataSourceSchemaOptionDraft } from "$lib/notes/database/data-source-schema";
  import {
    NOTES_DATA_SOURCE_MAX_OPTIONS,
    notesDataSourceAddOption,
    notesDataSourceDeleteOption,
    notesDataSourceMoveOption,
    notesDataSourceRecolorOption,
    notesDataSourceRenameOption,
    notesDataSourceSetDefaultOption,
    notesDataSourceStepOption,
    notesDataSourceVisibleOptions,
    type NotesDataSourceOptionEdit,
    type NotesDataSourceOptionEditor,
    type NotesDataSourceOptionPlacement,
    type NotesDataSourceOptionProperty,
  } from "$lib/notes/database/property-options";
  import { notesDatabaseViewDropPlacement } from "$lib/notes/database/view-order";
  import {
    NOTES_DATA_SOURCE_SELECT_COLORS,
    NOTES_DATA_SOURCE_STATUS_GROUPS,
    type NotesDataSourceSelectColor,
    type NotesDataSourceStatusGroup,
  } from "$lib/notes/types";
  import { cn } from "$lib/utils";
  import NotesDatabaseOptionBadge from "./NotesDatabaseOptionBadge.svelte";

  /**
   * Edits the options of a select, multi-select, or status property: add, rename, recolor, reorder, delete, and the
   * status default. Every change is handed to `onEdit` right away; there is no separate save step.
   */
  let { property, disabled = false, onEdit }: {
    property: NotesDataSourceOptionProperty;
    disabled?: boolean;
    /** Applies one edit to the newest saved property and saves it, resolving with the edit's outcome. */
    onEdit: (edit: NotesDataSourceOptionEditor) => Promise<NotesDataSourceOptionEdit["status"]>;
  } = $props();

  const COLOR_LABEL_KEYS = {
    default: "notes.blockColor.default",
    gray: "notes.blockColor.gray",
    brown: "notes.blockColor.brown",
    orange: "notes.blockColor.orange",
    yellow: "notes.blockColor.yellow",
    green: "notes.blockColor.green",
    blue: "notes.blockColor.blue",
    purple: "notes.blockColor.purple",
    pink: "notes.blockColor.pink",
    red: "notes.blockColor.red",
  } as const satisfies Record<NotesDataSourceSelectColor, string>;

  type DropTarget = { optionId: string; placement: NotesDataSourceOptionPlacement } | { group: NotesDataSourceStatusGroup };

  const { t } = getLocalization();
  const settingsRow = getCollectionSettingsNavigation() !== undefined;
  const status = $derived(property.type === "status");
  const visible = $derived(notesDataSourceVisibleOptions(property));
  const reorderable = $derived(!disabled && visible.length > (status ? 0 : 1));
  let root: HTMLDivElement | undefined = $state();
  let addInput: HTMLInputElement | undefined = $state();
  /** The group a new option is being typed for; select and multi-select options use the first group. */
  let adding: NotesDataSourceStatusGroup | null = $state(null);
  let newName = $state("");
  let error: string | null = $state(null);
  let dragged: string | null = $state(null);
  let dropTarget: DropTarget | null = $state(null);

  function colorStyle(color: NotesDataSourceSelectColor): string {
    return `${notesBlockColorStyle(color)}; background-color: color-mix(in srgb, var(--notes-block-color) 30%, var(--card));`;
  }

  /** Run one edit and explain the outcomes people can fix, such as a name another option already has. */
  async function edit(change: NotesDataSourceOptionEditor): Promise<NotesDataSourceOptionEdit["status"]> {
    error = null;
    const outcome = await onEdit(change);
    if (outcome === "duplicate") error = t("collections.property.optionExists");
    else if (outcome === "limit") error = t("collections.property.optionLimit", NOTES_DATA_SOURCE_MAX_OPTIONS);
    return outcome;
  }

  async function startAdding(group: NotesDataSourceStatusGroup): Promise<void> {
    if (disabled) return;
    adding = group;
    newName = "";
    error = null;
    await tick();
    addInput?.focus({ preventScroll: true });
  }

  /**
   * Add the typed option. Enter keeps the field open for the next option; leaving the field closes it.
   *
   * @param keepOpen Whether the field stays open after the option is added.
   */
  async function commitNew(keepOpen: boolean): Promise<void> {
    const group = adding;
    const name = newName;
    if (!group) return;
    if (!name.trim()) {
      if (!keepOpen) adding = null;
      return;
    }
    newName = "";
    const outcome = await edit((current) => notesDataSourceAddOption(current, name, group));
    if (outcome === "changed") {
      if (!keepOpen && !newName) adding = null;
    } else if (!newName) {
      newName = name;
    }
  }

  function rename(optionId: string, name: string): void {
    void edit((current) => notesDataSourceRenameOption(current, optionId, name));
  }

  /** Move an option one step with the arrow keys on its handle, keeping focus on the handle when it changes group. */
  async function stepWithKeyboard(event: KeyboardEvent, optionId: string): Promise<void> {
    if (event.key !== "ArrowUp" && event.key !== "ArrowDown") return;
    event.preventDefault();
    if (!reorderable) return;
    await edit((current) => notesDataSourceStepOption(current, optionId, event.key === "ArrowUp" ? -1 : 1));
    await tick();
    root?.querySelector<HTMLElement>(`[data-option-handle="${CSS.escape(optionId)}"]`)?.focus({ preventScroll: true });
  }

  function startDrag(event: DragEvent, optionId: string): void {
    if (!reorderable) {
      event.preventDefault();
      return;
    }
    dragged = optionId;
    error = null;
    event.dataTransfer?.setData("text/plain", optionId);
    if (event.dataTransfer) event.dataTransfer.effectAllowed = "move";
  }

  function dragOverOption(event: DragEvent & { currentTarget: HTMLElement }, optionId: string): void {
    if (!dragged) return;
    event.preventDefault();
    if (event.dataTransfer) event.dataTransfer.dropEffect = "move";
    dropTarget = dragged === optionId ? null : {
      optionId,
      placement: notesDatabaseViewDropPlacement(event.currentTarget.getBoundingClientRect(), { x: event.clientX, y: event.clientY }, "y"),
    };
  }

  /** Let a status option land at the end of a group, which is the only way into a group without options. */
  function dragOverGroup(event: DragEvent, group: NotesDataSourceStatusGroup): void {
    if (!dragged) return;
    event.preventDefault();
    if (event.dataTransfer) event.dataTransfer.dropEffect = "move";
    dropTarget = { group };
  }

  function drop(event: DragEvent): void {
    if (!dragged) return;
    event.preventDefault();
    const moved = dragged;
    const target = dropTarget;
    clearDrag();
    if (!target) return;
    void edit((current) => "group" in target
      ? notesDataSourceMoveOption(current, moved, null, target.group)
      : notesDataSourceMoveOption(current, moved, target));
  }

  function clearDrag(): void {
    dragged = null;
    dropTarget = null;
  }

  function dropMarker(optionId: string): NotesDataSourceOptionPlacement | null {
    return dropTarget && "optionId" in dropTarget && dropTarget.optionId === optionId ? dropTarget.placement : null;
  }
</script>

{#snippet header(label: string, group: NotesDataSourceStatusGroup, addLabel: string)}
  <div class={cn("flex min-w-0 items-center gap-1 rounded-floating-item", dropTarget && "group" in dropTarget && dropTarget.group === group && "bg-accent")} role="presentation"
    ondragover={(event) => dragOverGroup(event, group)} ondrop={drop}>
    <p class="menu-label min-w-0 flex-1 truncate">{label}</p>
    <button type="button" class="collection-menu-control flex size-6 shrink-0 items-center justify-center rounded-floating-item text-muted-foreground hover:bg-accent hover:text-foreground disabled:cursor-not-allowed"
      data-collection-menu-keep-open aria-label={addLabel} {disabled} onclick={() => { void startAdding(group); }}>
      <Plus class="size-3.5" strokeWidth={1.75} aria-hidden="true" />
    </button>
  </div>
  {#if adding === group}
    <input bind:this={addInput} class="field mx-0.5 mb-0.5 w-[calc(100%-0.25rem)]" aria-label={t("collections.property.optionName")} placeholder={t("collections.property.newOption")}
      bind:value={newName} onblur={() => { void commitNew(false); }}
      onkeydown={(event) => {
        if (event.key !== "Enter" || event.isComposing) return;
        event.preventDefault();
        void commitNew(true);
      }} />
  {/if}
{/snippet}

{#snippet optionRow(option: NotesDataSourceSchemaOptionDraft)}
  {@const isDefault = status && property.statusDefaultOptionId === option.id}
  {@const marker = dropMarker(option.id)}
  <div class={cn("relative flex min-w-0 items-center", dragged === option.id && "opacity-50")} role="presentation" data-option-id={option.id}
    draggable={reorderable} ondragstart={(event) => startDrag(event, option.id)} ondragover={(event) => dragOverOption(event, option.id)} ondrop={drop} ondragend={clearDrag}>
    {#if marker}
      <span class={cn("pointer-events-none absolute inset-x-1 h-0.5 rounded-full bg-primary", marker === "before" ? "top-0" : "bottom-0")} aria-hidden="true"></span>
    {/if}
    <button type="button" class="collection-menu-control flex w-6 shrink-0 cursor-grab items-center justify-center self-stretch rounded-floating-item text-muted-foreground active:cursor-grabbing aria-disabled:cursor-not-allowed pointer-coarse:hidden"
      data-collection-menu-keep-open data-collection-menu-row-control data-option-handle={option.id} aria-label={t("collections.property.dragOption", option.name)} aria-disabled={!reorderable}
      onkeydown={(event) => { void stepWithKeyboard(event, option.id); }}>
      <GripVertical class="size-3.5" strokeWidth={1.75} aria-hidden="true" />
    </button>
    <CollectionMenu label={option.name} ariaLabel={t("collections.property.editOption", option.name)} kind="properties" fullWidth row iconOnly showHeader={false}
      rootClass="min-w-0 flex-1" triggerClass="pl-1" summary="" {disabled}>
      {#snippet leading()}
        <NotesDatabaseOptionBadge label={option.name} color={option.color} />
        {#if isDefault}<span class="shrink-0 text-[0.666667rem] font-medium tracking-wide text-muted-foreground uppercase">{t("collections.property.defaultMarker")}</span>{/if}
      {/snippet}
      <CollectionMenuTextField label={t("collections.property.optionName")} value={option.name} hideLabel initialFocus {disabled}
        onCommit={(name) => rename(option.id, name)} />
      {#if status}
        <CollectionMenuItem icon={CircleCheck} label={t("collections.property.setDefault")} checked={isDefault} {disabled}
          onclick={() => { void edit((current) => notesDataSourceSetDefaultOption(current, option.id)); }} />
      {/if}
      <CollectionMenuItem icon={Trash2} label={t("collections.property.deleteOption")} destructive {disabled}
        onclick={() => { void edit((current) => notesDataSourceDeleteOption(current, option.id)); }} />
      <CollectionMenuSeparator />
      <p class="menu-label">{t("collections.property.colors")}</p>
      {#each NOTES_DATA_SOURCE_SELECT_COLORS as color (color)}
        <button type="button" class="menu-item min-w-0" data-collection-settings-row={settingsRow ? "" : undefined} aria-pressed={option.color === color} {disabled}
          onclick={() => { void edit((current) => notesDataSourceRecolorOption(current, option.id, color)); }}>
          <span class="size-3.5 shrink-0 rounded-sm" style={colorStyle(color)} aria-hidden="true"></span>
          <span class="min-w-0 flex-1 truncate">{t(COLOR_LABEL_KEYS[color])}</span>
          {#if option.color === color}<Check class="size-3.5 shrink-0" aria-hidden="true" />{/if}
        </button>
      {/each}
    </CollectionMenu>
  </div>
{/snippet}

<div bind:this={root} class="grid min-w-0 gap-0" data-notes-property-options>
  {#if status}
    {#each NOTES_DATA_SOURCE_STATUS_GROUPS as group (group)}
      {@const groupLabel = t("notes.databaseSchemaStatusGroup", group)}
      {@render header(groupLabel, group, t("notes.databaseSchemaAddStatusOption", groupLabel))}
      {#each visible.filter((option) => option.group === group) as option (option.id)}
        {@render optionRow(option)}
      {/each}
    {/each}
  {:else}
    {@render header(t("collections.property.options"), "To-do", t("collections.property.addOption"))}
    {#each visible as option (option.id)}
      {@render optionRow(option)}
    {:else}
      {#if adding === null}<p class="px-2 py-1 text-muted-foreground">{t("collections.property.noOptions")}</p>{/if}
    {/each}
  {/if}
  {#if error}<Notice class="px-2 py-1 text-panel-detail" message={error} />{/if}
</div>
