<script lang="ts">
  import Check from "@lucide/svelte/icons/check";
  import { tick } from "svelte";
  import { floatPanel } from "$lib/components/contacts/float-panel";
  import { FLOATING_WIDTH } from "$lib/components/ui/floating-width";
  import Notice from "$lib/components/ui/Notice.svelte";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import type { NotesDataSourceSchemaOptionDraft } from "$lib/notes/database/data-source-schema";
  import { NOTES_DATA_SOURCE_MAX_OPTIONS, notesDataSourceOptionName, type NotesDataSourceOptionEdit } from "$lib/notes/database/property-options";
  import { notesDatabaseTableOptionNames, type NotesDatabaseTableColumn } from "$lib/notes/database/table";
  import { NOTES_DATA_SOURCE_STATUS_GROUPS, type NotesPage } from "$lib/notes/types";
  import { cn } from "$lib/utils";
  import NotesDatabaseOptionBadge from "../NotesDatabaseOptionBadge.svelte";
  import { OPENING_MAY_FOCUS_TEXT } from "$lib/mobile/keyboard";

  /**
   * A select, status, or multi-select value on a row page: the chosen options show as pills, and a click opens a
   * searchable option list. Picks save at once, in the order they were made. A single choice closes the list, while a
   * multi-select stays open to toggle more, and typing a new name offers to create that option.
   */
  let { row, column, rowIndex, columnIndex, options, onSave, onCreate, onNavigate, onEditingChange = () => {} }: {
    row: NotesPage;
    column: NotesDatabaseTableColumn;
    rowIndex: number;
    columnIndex: number;
    /** The property's options in display order, with the status group of each. */
    options: readonly NotesDataSourceSchemaOptionDraft[];
    /** Saves option names: one name or null for a single choice, every chosen name for a multi-select. */
    onSave: (value: string | string[] | null) => Promise<boolean>;
    /** Adds an option with the given name; the create row is hidden without it. */
    onCreate?: (name: string) => Promise<NotesDataSourceOptionEdit["status"]>;
    onNavigate: (event: KeyboardEvent, rowIndex: number, columnIndex: number) => void;
    onEditingChange?: (editing: boolean) => void;
  } = $props();

  type PickerItem = { kind: "option"; option: NotesDataSourceSchemaOptionDraft } | { kind: "create"; name: string };
  interface PickerGroup {
    label: string | null;
    items: { item: PickerItem; index: number }[];
  }

  const { t } = getLocalization();
  const pickerId = $props.id();
  const multiple = $derived(column.type === "multi_select");
  const saved = $derived(notesDatabaseTableOptionNames(row, column));

  let trigger: HTMLButtonElement | undefined = $state();
  let searchInput: HTMLInputElement | undefined = $state();
  let open = $state(false);
  let query = $state("");
  let activeIndex = $state(0);
  let draft = $state.raw<string[]>([]);
  let pendingSaves = $state(0);
  let creating = $state(false);
  let error = $state<string | null>(null);
  let latestSave = 0;
  let saveChain: Promise<void> = Promise.resolve();

  /** The chosen names on screen: the newest pick while saves are in flight, otherwise the saved value. */
  const shown = $derived(pendingSaves > 0 ? draft : saved);
  const queryKey = $derived(query.trim().toLowerCase());
  const matches = $derived(options.filter((option) => option.name.toLowerCase().includes(queryKey)));
  const createName = $derived(notesDataSourceOptionName(query));
  const canCreate = $derived(Boolean(onCreate) && createName !== ""
    && !options.some((option) => option.name.trim().toLowerCase() === createName.toLowerCase()));
  const items = $derived<PickerItem[]>([
    ...(column.type === "status"
      ? NOTES_DATA_SOURCE_STATUS_GROUPS.flatMap((group) => matches.filter((option) => option.group === group))
      : matches).map((option) => ({ kind: "option" as const, option })),
    ...(canCreate ? [{ kind: "create" as const, name: createName }] : []),
  ]);
  const groups = $derived.by<PickerGroup[]>(() => {
    const indexed = items.map((item, index) => ({ item, index }));
    if (column.type !== "status") return [{ label: null, items: indexed }];
    const grouped: PickerGroup[] = NOTES_DATA_SOURCE_STATUS_GROUPS.map((group) => ({
      label: t("notes.databaseSchemaStatusGroup", group),
      items: indexed.filter(({ item }) => item.kind === "option" && item.option.group === group),
    }));
    return [...grouped.filter((group) => group.items.length > 0), { label: null, items: indexed.filter(({ item }) => item.kind === "create") }];
  });

  $effect(() => {
    void queryKey;
    activeIndex = 0;
  });

  $effect(() => {
    if (!open || !searchInput || !OPENING_MAY_FOCUS_TEXT) return;
    const input = searchInput;
    void tick().then(() => input.focus({ preventScroll: true }));
  });

  function colorOf(name: string): string {
    const key = name.toLowerCase();
    return options.find((option) => option.name.toLowerCase() === key)?.color ?? "default";
  }

  function isChosen(name: string): boolean {
    const key = name.toLowerCase();
    return shown.some((chosen) => chosen.toLowerCase() === key);
  }

  function itemId(index: number): string {
    return `${pickerId}-option-${index}`;
  }

  function show(): void {
    if (open) return;
    query = "";
    error = null;
    open = true;
    onEditingChange(true);
  }

  /** Close the list, returning focus to the value when the close came from the keyboard or a single pick. */
  function close(refocus: boolean): void {
    if (!open) return;
    open = false;
    onEditingChange(false);
    if (refocus) trigger?.focus({ preventScroll: true });
  }

  /**
   * Show new names and save them after any earlier save of this value, so saves land in the order they were picked.
   * Only the newest save reports a failure and restores the saved names.
   */
  function commit(next: string[]): void {
    draft = next;
    error = null;
    const value = multiple ? next : next[0] ?? null;
    const sequence = ++latestSave;
    pendingSaves += 1;
    saveChain = saveChain.then(async () => {
      let succeeded = false;
      try {
        succeeded = await onSave(value);
      } catch (caught: unknown) {
        console.warn("Save database option failed", caught);
      }
      if (!succeeded && sequence === latestSave) {
        error = t("notes.databaseRowPageOptionSaveFailed");
        draft = saved;
      }
    }).finally(() => { pendingSaves -= 1; });
  }

  /** Toggle a name: a multi-select keeps the list open for more, a single choice closes it. */
  function choose(name: string): void {
    const chosen = isChosen(name);
    const key = name.toLowerCase();
    if (multiple) {
      commit(chosen ? shown.filter((entry) => entry.toLowerCase() !== key) : [...shown, name]);
      query = "";
      searchInput?.focus({ preventScroll: true });
      return;
    }
    commit(chosen ? [] : [name]);
    close(true);
  }

  async function create(name: string): Promise<void> {
    if (!onCreate || creating) return;
    creating = true;
    error = null;
    try {
      const status = await onCreate(name);
      if (status === "changed" || status === "duplicate") {
        if (!isChosen(name)) choose(name);
        else query = "";
      } else if (status === "limit") {
        error = t("collections.property.optionLimit", NOTES_DATA_SOURCE_MAX_OPTIONS);
      } else {
        error = t("notes.databaseRowPageOptionSaveFailed");
      }
    } finally {
      creating = false;
    }
  }

  function pick(item: PickerItem | undefined): void {
    if (!item) return;
    if (item.kind === "create") void create(item.name);
    else choose(item.option.name);
  }

  function moveActive(step: -1 | 1): void {
    if (items.length === 0) return;
    activeIndex = (activeIndex + step + items.length) % items.length;
    void tick().then(() => document.getElementById(itemId(activeIndex))?.scrollIntoView({ block: "nearest" }));
  }

  /** List keys from the search field; the panel's keyboard layer handles Escape and Tab. */
  function handleKeydown(event: KeyboardEvent): void {
    if (event.key === "ArrowDown" || event.key === "ArrowUp") {
      event.preventDefault();
      moveActive(event.key === "ArrowDown" ? 1 : -1);
    } else if (event.key === "Enter" && !event.isComposing) {
      event.preventDefault();
      pick(items[activeIndex]);
    } else if (event.key === "Backspace" && multiple && query === "" && shown.length > 0) {
      event.preventDefault();
      commit(shown.slice(0, -1));
    }
  }
</script>

<button bind:this={trigger} type="button"
  class="flex min-h-8 w-full min-w-0 cursor-pointer flex-wrap items-center gap-1 rounded-sm px-1.5 py-1 text-left outline-none focus-visible:bg-accent/60"
  aria-label={column.name} aria-haspopup="dialog" aria-expanded={open}
  data-table-cell="true" data-collection-cell-primary data-row-index={rowIndex} data-column-index={columnIndex}
  onclick={() => { if (open) close(false); else show(); }}
  onkeydown={(event) => onNavigate(event, rowIndex, columnIndex)}>
  {#each shown as name, index (index)}
    <NotesDatabaseOptionBadge label={name} color={colorOf(name)} />
  {:else}
    <span class="truncate text-muted-foreground">{t("notes.databaseTableEmptyCell")}</span>
  {/each}
</button>

{#if open && trigger}
  <div class="surface-floating fixed z-80 flex flex-col overflow-hidden outline-none" role="dialog" aria-label={column.name} tabindex="-1"
    data-floating-root data-app-floating-surface
    use:floatPanel={{ anchor: trigger, width: FLOATING_WIDTH.md, horizontalAlign: "start", onDismiss: () => close(false), onKeydown: handleKeydown }}>
    <label class="field m-2 mb-1 flex items-center">
      <input bind:this={searchInput} bind:value={query} type="search" class="field-bare text-foreground"
        role="combobox" aria-expanded="true" aria-autocomplete="list" aria-controls={`${pickerId}-list`}
        aria-activedescendant={items[activeIndex] ? itemId(activeIndex) : undefined}
        aria-label={t("notes.databaseRowPageOptionSearch")} placeholder={t("notes.databaseRowPageOptionSearch")} />
    </label>
    <div id={`${pickerId}-list`} class="option-list min-h-0 overflow-y-auto px-1 pb-1" role="listbox" aria-label={column.name}
      aria-multiselectable={multiple} aria-busy={creating}>
      {#if items.length === 0}
        <p class="px-2 py-1.5 text-panel-detail text-muted-foreground">
          {options.length === 0 ? t("collections.property.noOptions") : t("notes.databaseRowPageOptionEmpty")}
        </p>
      {/if}
      {#each groups as group, groupIndex (groupIndex)}
        {#if group.label}<div class="menu-label truncate" aria-hidden="true">{group.label}</div>{/if}
        {#each group.items as { item, index } (item.kind === "option" ? item.option.id : "create")}
          <!-- The search field owns keyboard choice through aria-activedescendant; options take the pointer. -->
          <!-- svelte-ignore a11y_click_events_have_key_events -->
          <div id={itemId(index)} class={cn("menu-item min-w-0 cursor-pointer", creating && "pointer-events-none opacity-60")}
            role="option" tabindex="-1" aria-selected={item.kind === "option" && isChosen(item.option.name)}
            data-highlighted={index === activeIndex ? "" : undefined}
            onpointermove={() => { activeIndex = index; }}
            onpointerdown={(event) => event.preventDefault()}
            onclick={() => pick(item)}>
            {#if item.kind === "option"}
              <span class="flex min-w-0 flex-1"><NotesDatabaseOptionBadge label={item.option.name} color={item.option.color} /></span>
              {#if isChosen(item.option.name)}<Check class="size-4 shrink-0 text-muted-foreground" aria-hidden="true" />{/if}
            {:else}
              <span class="shrink-0 text-muted-foreground">{t("notes.databaseRowPageOptionCreate")}</span>
              <span class="flex min-w-0 flex-1"><NotesDatabaseOptionBadge label={item.name} color="gray" /></span>
            {/if}
          </div>
        {/each}
      {/each}
    </div>
    {#if error}<Notice class="border-t border-border px-3 py-1.5 text-panel-detail" message={error} />{/if}
  </div>
{/if}

<style>
  .option-list {
    max-height: min(20rem, 50vh);
  }
</style>
