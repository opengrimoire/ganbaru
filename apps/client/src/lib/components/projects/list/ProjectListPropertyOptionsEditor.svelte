<script lang="ts">
  import { tick } from "svelte";
  import GripVertical from "@lucide/svelte/icons/grip-vertical";
  import Plus from "@lucide/svelte/icons/plus";
  import Trash2 from "@lucide/svelte/icons/trash-2";
  import CollectionMenu from "$lib/components/collections/CollectionMenu.svelte";
  import CollectionMenuItem from "$lib/components/collections/CollectionMenuItem.svelte";
  import CollectionMenuTextField from "$lib/components/collections/CollectionMenuTextField.svelte";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import { useProjects } from "$lib/stores/projects.svelte";
  import { PROJECT_CUSTOM_FIELD_OPTION_DRAG_DATA_TYPE } from "$lib/projects/custom-fields";
  import { createProjectSettingsReorderController } from "$lib/projects/settings/reorder-controller.svelte";
  import type { ProjectCustomField, ProjectCustomFieldOption } from "$lib/projects/types";
  import { cn } from "$lib/utils";

  /**
   * Edits the options of a Projects select, multi-select, or status field: add, rename, reorder, and delete. Every
   * change is saved right away; there is no separate save step.
   */
  let { field, disabled = false }: { field: ProjectCustomField; disabled?: boolean } = $props();

  const projects = useProjects();
  const { t } = getLocalization();
  const options = $derived(projects.customFieldOptionsForField(field.id));
  let root: HTMLDivElement | undefined = $state();
  let addInput: HTMLInputElement | undefined = $state();
  let adding = $state(false);
  let newName = $state("");
  let saving = $state(false);
  let error: string | null = $state(null);
  const reorder = createProjectSettingsReorderController<ProjectCustomFieldOption>({
    dataType: PROJECT_CUSTOM_FIELD_OPTION_DRAG_DATA_TYPE,
    getEntries: () => projects.customFieldOptionsForField(field.id),
    moveEntry: projects.moveCustomFieldOption,
    setError: (message) => { error = message; },
    reorderFailedMessage: () => t("projects.customFields.optionReorderFailed"),
    saveFailedMessage: (message) => t("projects.customFields.optionSaveFailed", message),
  });
  const busy = $derived(disabled || saving || reorder.pending);
  const reorderable = $derived(!busy && options.length > 1);

  /** Whether another option of this field already uses the name, ignoring case and surrounding spaces. */
  function nameTaken(name: string, exceptId?: string): boolean {
    const key = name.trim().toLocaleLowerCase();
    return options.some((option) => option.id !== exceptId && option.name.toLocaleLowerCase() === key);
  }

  /**
   * Run one option write at a time and show its failure in the editor.
   *
   * @returns Whether the write succeeded.
   */
  async function write(action: () => Promise<unknown>, failure: (message: string) => string): Promise<boolean> {
    if (busy) return false;
    saving = true;
    error = null;
    try {
      await action();
      return true;
    } catch (cause: unknown) {
      error = failure(cause instanceof Error ? cause.message : String(cause));
      return false;
    } finally { saving = false; }
  }

  async function startAdding(): Promise<void> {
    if (busy) return;
    adding = true;
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
    if (!adding) return;
    const name = newName.trim();
    if (!name) {
      if (!keepOpen) adding = false;
      return;
    }
    if (nameTaken(name)) {
      error = t("projects.customFields.optionNameExists");
      return;
    }
    newName = "";
    const added = await write(() => projects.addCustomFieldOption(field.id, name), (message) => t("projects.customFields.optionSaveFailed", message));
    if (added) {
      if (!keepOpen && !newName) adding = false;
    } else if (!newName) {
      newName = name;
    }
  }

  function rename(option: ProjectCustomFieldOption, name: string): void {
    if (nameTaken(name, option.id)) {
      error = t("projects.customFields.optionNameExists");
      return;
    }
    void write(() => projects.updateCustomFieldOption(option, { name }), (message) => t("projects.customFields.optionSaveFailed", message));
  }

  function remove(option: ProjectCustomFieldOption): void {
    void write(() => projects.removeCustomFieldOption(option.id), (message) => t("projects.customFields.optionDeleteFailed", message));
  }

  /** Move an option one step with the arrow keys on its handle, keeping focus on the handle. */
  async function stepWithKeyboard(event: KeyboardEvent, option: ProjectCustomFieldOption): Promise<void> {
    if (event.key !== "ArrowUp" && event.key !== "ArrowDown") return;
    event.preventDefault();
    if (!reorderable) return;
    const target = options.findIndex((candidate) => candidate.id === option.id) + (event.key === "ArrowUp" ? -1 : 1);
    if (target < 0 || target >= options.length) return;
    await reorder.moveToIndex(option.id, target);
    await tick();
    const handles = root?.querySelectorAll<HTMLElement>("[data-option-handle]") ?? [];
    Array.from(handles).find((handle) => handle.dataset.optionHandle === option.id)?.focus({ preventScroll: true });
  }

  function startDrag(event: DragEvent, option: ProjectCustomFieldOption): void {
    if (!reorderable) {
      event.preventDefault();
      return;
    }
    error = null;
    reorder.start(event, option);
  }
</script>

<div bind:this={root} class="grid min-w-0 gap-0" data-project-property-options>
  <div class="flex min-w-0 items-center gap-1">
    <p class="menu-label min-w-0 flex-1 truncate">{t("collections.property.options")}</p>
    <button type="button" class="collection-menu-control flex size-6 shrink-0 items-center justify-center rounded-floating-item text-muted-foreground hover:bg-accent hover:text-foreground disabled:cursor-not-allowed"
      data-collection-menu-keep-open aria-label={t("collections.property.addOption")} disabled={busy} onclick={() => { void startAdding(); }}>
      <Plus class="size-3.5" strokeWidth={1.75} aria-hidden="true" />
    </button>
  </div>
  {#if adding}
    <input bind:this={addInput} class="field mx-0.5 mb-0.5 w-[calc(100%-0.25rem)]" aria-label={t("collections.property.optionName")} placeholder={t("collections.property.newOption")}
      bind:value={newName} onblur={() => { void commitNew(false); }}
      onkeydown={(event) => {
        if (event.key !== "Enter" || event.isComposing) return;
        event.preventDefault();
        void commitNew(true);
      }} />
  {/if}
  {#each options as option (option.id)}
    <div class={cn("relative flex min-w-0 items-center", reorder.draggedId === option.id && "opacity-50")} role="presentation" data-option-id={option.id}
      draggable={reorderable} ondragstart={(event) => startDrag(event, option)} ondragover={(event) => reorder.dragOver(event, option, event.currentTarget)}
      ondrop={(event) => { void reorder.drop(event, option); }} ondragend={reorder.clear}>
      {#if reorder.markerVisible(option.id, "before")}<span class="pointer-events-none absolute inset-x-1 top-0 h-0.5 rounded-full bg-primary" aria-hidden="true"></span>{/if}
      {#if reorder.markerVisible(option.id, "after")}<span class="pointer-events-none absolute inset-x-1 bottom-0 h-0.5 rounded-full bg-primary" aria-hidden="true"></span>{/if}
      <button type="button" class="collection-menu-control flex w-6 shrink-0 cursor-grab items-center justify-center self-stretch rounded-floating-item text-muted-foreground active:cursor-grabbing aria-disabled:cursor-not-allowed pointer-coarse:hidden"
        data-collection-menu-keep-open data-collection-menu-row-control data-option-handle={option.id} aria-label={t("collections.property.dragOption", option.name)} aria-disabled={!reorderable}
        onkeydown={(event) => { void stepWithKeyboard(event, option); }}>
        <GripVertical class="size-3.5" strokeWidth={1.75} aria-hidden="true" />
      </button>
      <CollectionMenu label={option.name} ariaLabel={t("collections.property.editOption", option.name)} kind="properties" fullWidth row iconOnly showHeader={false}
        rootClass="min-w-0 flex-1" triggerClass="pl-1" summary="" disabled={busy}>
        {#snippet leading()}<span class="min-w-0 truncate rounded-sm bg-muted px-1.5 py-0.5">{option.name}</span>{/snippet}
        <CollectionMenuTextField label={t("collections.property.optionName")} value={option.name} hideLabel initialFocus disabled={busy}
          onCommit={(name) => rename(option, name)} />
        <CollectionMenuItem icon={Trash2} label={t("collections.property.deleteOption")} destructive disabled={busy} onclick={() => remove(option)} />
      </CollectionMenu>
    </div>
  {:else}
    {#if !adding}<p class="px-2 py-1 text-muted-foreground">{t("collections.property.noOptions")}</p>{/if}
  {/each}
  {#if error}<p role="alert" class="px-2 py-1 text-panel-detail text-destructive">{error}</p>{/if}
</div>
