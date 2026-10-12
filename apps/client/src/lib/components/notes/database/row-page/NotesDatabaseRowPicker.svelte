<script lang="ts">
  import Plus from "@lucide/svelte/icons/plus";
  import Search from "@lucide/svelte/icons/search";
  import { tick } from "svelte";
  import { SvelteSet } from "svelte/reactivity";
  import { listNotesDataSourceRowPages } from "$lib/api/notes";
  import { floatPanel } from "$lib/components/contacts/float-panel";
  import NotesPageIcon from "$lib/components/notes/pages/NotesPageIcon.svelte";
  import { FLOATING_WIDTH } from "$lib/components/ui/floating-width";
  import Loading from "$lib/components/ui/Loading.svelte";
  import Notice from "$lib/components/ui/Notice.svelte";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import { notesDatabaseRowSearch } from "$lib/notes/database/row-page-properties";
  import { notesDatabaseRowTitle } from "$lib/notes/database/table";
  import type { NotesPage } from "$lib/notes/types";
  import { cn } from "$lib/utils";
  import { scrollEdgeFadeAction } from "$lib/utils/scroll-edge-fade";

  /**
   * Floating "Link or create a page" picker over the rows of one data source. Typing filters by title, the arrow keys
   * and Enter choose, and the last option creates a page with the typed title. Picked rows leave the list at once, so
   * several can be linked in a row unless the picker closes on its first pick.
   */
  let {
    anchor,
    dataSourceId,
    sourceName,
    exclude = [],
    closeOnPick = false,
    onPick,
    onCreate,
    onClose,
  }: {
    anchor: HTMLElement;
    /** Source whose rows are listed and where new pages are created. */
    dataSourceId: string;
    sourceName: string;
    /** Rows that cannot be picked, such as the row itself or rows already linked. */
    exclude?: readonly string[];
    closeOnPick?: boolean;
    /** Links a row; a rejection shows its message in the picker. */
    onPick: (row: NotesPage) => Promise<void>;
    /** Creates and links a page with the typed title; the create option is hidden without it. */
    onCreate?: (title: string) => Promise<void>;
    onClose: () => void;
  } = $props();

  type PickerOption = { kind: "row"; row: NotesPage } | { kind: "create" };

  /** Rendered rows are capped so a large source stays responsive; searching reaches the rest. */
  const MAX_RENDERED_ROWS = 50;
  const { t } = getLocalization();
  const listboxId = `notes-row-picker-${crypto.randomUUID()}`;

  let searchInput: HTMLInputElement | undefined = $state();
  let listbox: HTMLElement | undefined = $state();
  let query = $state("");
  let rows = $state<NotesPage[]>([]);
  let loading = $state(true);
  let loadError = $state<string | null>(null);
  let error = $state<string | null>(null);
  let pending = $state(false);
  let activeIndex = $state(0);
  let loadRequest = 0;
  /** Rows picked here, hidden before the caller's exclusions catch up. */
  const pickedIds = new SvelteSet<string>();

  const excluded = $derived(new Set([...exclude, ...pickedIds]));
  const matches = $derived(notesDatabaseRowSearch(rows.filter((row) => !excluded.has(row.id)), query, titleOf));
  const options = $derived<PickerOption[]>([
    ...matches.slice(0, MAX_RENDERED_ROWS).map((row) => ({ kind: "row" as const, row })),
    ...(onCreate ? [{ kind: "create" as const }] : []),
  ]);
  const activeOptionId = $derived(options[activeIndex] ? optionId(activeIndex) : undefined);
  const trimmedQuery = $derived(query.trim());

  $effect(() => {
    void dataSourceId;
    void load();
  });

  $effect(() => {
    void query;
    activeIndex = 0;
  });

  $effect(() => {
    void tick().then(() => searchInput?.focus({ preventScroll: true }));
  });

  function titleOf(row: NotesPage): string {
    return notesDatabaseRowTitle(row, "");
  }

  function optionId(index: number): string {
    return `${listboxId}-${index}`;
  }

  function messageOf(caught: unknown): string {
    return caught instanceof Error ? caught.message : String(caught);
  }

  async function load(): Promise<void> {
    const request = ++loadRequest;
    loadError = null;
    try {
      const loaded = await listNotesDataSourceRowPages(dataSourceId);
      if (request !== loadRequest) return;
      rows = loaded;
    } catch (caught) {
      if (request === loadRequest) loadError = messageOf(caught);
    } finally {
      if (request === loadRequest) loading = false;
    }
  }

  /** Loads the rows again after a failure, rejecting while they are still unavailable. */
  async function retryLoad(): Promise<void> {
    await load();
    if (loadError) throw new Error(loadError);
  }

  async function choose(option: PickerOption | undefined): Promise<void> {
    if (!option || pending) return;
    pending = true;
    error = null;
    try {
      if (option.kind === "row") {
        await onPick(option.row);
        pickedIds.add(option.row.id);
      } else if (onCreate) {
        await onCreate(trimmedQuery);
        query = "";
        await load();
      }
      if (closeOnPick) {
        onClose();
        anchor.focus({ preventScroll: true });
        return;
      }
    } catch (caught) {
      error = messageOf(caught);
    } finally {
      pending = false;
    }
    activeIndex = Math.min(activeIndex, Math.max(0, options.length - 1));
    searchInput?.focus({ preventScroll: true });
  }

  function moveActive(step: -1 | 1): void {
    if (options.length === 0) return;
    activeIndex = (activeIndex + step + options.length) % options.length;
    void tick().then(() => {
      listbox?.querySelector<HTMLElement>(`#${CSS.escape(optionId(activeIndex))}`)?.scrollIntoView({ block: "nearest" });
    });
  }

  function handleKeydown(event: KeyboardEvent): void {
    if (event.key === "ArrowDown" || event.key === "ArrowUp") {
      event.preventDefault();
      moveActive(event.key === "ArrowDown" ? 1 : -1);
    } else if (event.key === "Enter" && !event.isComposing) {
      event.preventDefault();
      void choose(options[activeIndex]);
    }
  }
</script>

<div
  class="surface-floating fixed z-80 flex flex-col overflow-hidden outline-none"
  role="dialog"
  aria-label={t("notes.databaseRowPickerTitle")}
  tabindex="-1"
  data-floating-root
  data-app-floating-surface
  use:floatPanel={{ anchor, width: FLOATING_WIDTH.lg, horizontalAlign: "start", onDismiss: onClose, onKeydown: handleKeydown }}
>
  <label class="field m-2 mb-1 flex items-center gap-1.5">
    <Search class="size-3.5 shrink-0 text-muted-foreground" aria-hidden="true" />
    <input
      bind:this={searchInput}
      bind:value={query}
      type="search"
      class="field-bare text-foreground"
      role="combobox"
      aria-expanded="true"
      aria-autocomplete="list"
      aria-controls={listboxId}
      aria-activedescendant={activeOptionId}
      aria-label={t("notes.databaseRowPickerSearch")}
      placeholder={t("notes.databaseRowPickerSearch")}
    />
  </label>
  <div bind:this={listbox} id={listboxId} class="row-picker-list min-h-0 overflow-y-auto px-1 pb-1" role="listbox"
    aria-label={t("notes.databaseRowPickerIn", sourceName)} aria-busy={loading || pending} use:scrollEdgeFadeAction>
    <div class="menu-label truncate" aria-hidden="true">{t("notes.databaseRowPickerIn", sourceName)}</div>
    {#if loading}
      <Loading layout="rows" rows={3} label={t("notes.databaseRelationLoading")} />
    {:else if loadError}
      <Notice class="px-2 py-1.5 text-panel-detail" message={t("notes.databaseRowPickerLoadFailed", loadError)} copy={loadError} onRetry={retryLoad} />
    {:else if matches.length === 0}
      <p class="px-2 py-1.5 text-panel-detail text-muted-foreground">{t("notes.databaseRowPickerEmpty")}</p>
    {/if}
    {#each options as option, index (option.kind === "row" ? option.row.id : "create")}
      {@const title = option.kind === "row" ? titleOf(option.row) : ""}
      <!-- Options are chosen with the pointer here; the search field owns keyboard choice through aria-activedescendant. -->
      <!-- svelte-ignore a11y_click_events_have_key_events -->
      <div id={optionId(index)} class={cn("menu-item min-w-0 cursor-pointer", pending && "pointer-events-none opacity-60")}
        role="option" tabindex="-1" aria-selected={index === activeIndex} aria-disabled={pending}
        data-highlighted={index === activeIndex ? "" : undefined}
        onpointermove={() => { activeIndex = index; }}
        onpointerdown={(event) => event.preventDefault()}
        onclick={() => { void choose(option); }}>
        {#if option.kind === "row"}
          <NotesPageIcon icon={option.row.icon} size={16} class="shrink-0 text-muted-foreground" />
          <span class={cn("min-w-0 flex-1 truncate", !title && "text-muted-foreground")}>{title || t("notes.untitled")}</span>
        {:else}
          <Plus class="size-4 shrink-0 text-muted-foreground" aria-hidden="true" />
          <span class="min-w-0 flex-1 truncate">
            {trimmedQuery ? t("notes.databaseRowPickerCreate", trimmedQuery, sourceName) : t("notes.databaseRowPickerCreateUntitled", sourceName)}
          </span>
        {/if}
      </div>
    {/each}
  </div>
  {#if error}<Notice class="border-t border-border px-3 py-1.5 text-panel-detail" message={error} />{/if}
</div>

<style>
  .row-picker-list {
    max-height: min(20rem, 50vh);
  }
</style>
