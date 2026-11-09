<script lang="ts" module>
  /** Canvas geometry in logical pixels: a width and a height the room chooses between these bounds. */
  const MIN_CANVAS_W = 360;
  const MAX_CANVAS_W = 760;
  const MIN_CANVAS_H = 470;
  const MAX_CANVAS_H = 540;
  /** The bezel around the canvas, matching the other welcome scenes (`p-1.5`). */
  const BEZEL = 6;

  export const NOTES_SCENE_MIN_WIDTH = MIN_CANVAS_W + 2 * BEZEL;
  export const NOTES_SCENE_MAX_WIDTH = MAX_CANVAS_W + 2 * BEZEL;
  export const NOTES_SCENE_MIN_HEIGHT = MIN_CANVAS_H + 2 * BEZEL;
  export const NOTES_SCENE_MAX_HEIGHT = MAX_CANVAS_H + 2 * BEZEL;
</script>

<script lang="ts">
  import { untrack } from "svelte";
  import Check from "@lucide/svelte/icons/check";
  import ChevronDown from "@lucide/svelte/icons/chevron-down";
  import Columns3 from "@lucide/svelte/icons/columns-3";
  import Music from "@lucide/svelte/icons/eighth-notes";
  import Ellipsis from "@lucide/svelte/icons/ellipsis";
  import Feather from "@lucide/svelte/icons/feather";
  import FileText from "@lucide/svelte/icons/file-text";
  import GripVertical from "@lucide/svelte/icons/grip-vertical";
  import Heart from "@lucide/svelte/icons/heart";
  import PanelLeft from "@lucide/svelte/icons/panel-left";
  import PanelRight from "@lucide/svelte/icons/panel-right";
  import Plus from "@lucide/svelte/icons/plus";
  import Search from "@lucide/svelte/icons/search";
  import Share2 from "@lucide/svelte/icons/share-2";
  import SlidersHorizontal from "@lucide/svelte/icons/sliders-horizontal";
  import Star from "@lucide/svelte/icons/star";
  import Table2 from "@lucide/svelte/icons/table-2";
  import CollectionCell from "$lib/components/collections/CollectionCell.svelte";
  import CollectionPropertyIcon from "$lib/components/collections/CollectionPropertyIcon.svelte";
  import CollectionRow from "$lib/components/collections/CollectionRow.svelte";
  import NotesCalloutEmbeddedRow from "$lib/components/notes/blocks/NotesCalloutEmbeddedRow.svelte";
  import {
    notesSlashCommandHint,
    notesSlashCommandIcon,
    notesSlashCommandLabel,
    notesSlashMenuCatalog,
    notesSlashMenuGroups,
  } from "$lib/components/notes/blocks/slash-menu-presentation";
  import NotesPageIcon from "$lib/components/notes/pages/NotesPageIcon.svelte";
  import { FLOATING_WIDTH } from "$lib/components/ui/floating-width";
  import WorkspaceBreadcrumbTerminalIcon from "$lib/components/ui/WorkspaceBreadcrumbTerminalIcon.svelte";
  import WelcomeCaret from "$lib/components/vault/welcome/WelcomeCaret.svelte";
  import WelcomeCursor from "$lib/components/vault/welcome/WelcomeCursor.svelte";
  import WelcomeTitleIcon from "$lib/components/vault/welcome/WelcomeTitleIcon.svelte";
  import WelcomeTouch from "$lib/components/vault/welcome/WelcomeTouch.svelte";
  import { formatNumber } from "$lib/i18n/formatters";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import type { NotesCalloutLayer } from "$lib/notes/block-types/callout-layout";
  import { notesBlockColorStyle, notesBlockColorSwatchStyle } from "$lib/notes/blocks/color";
  import { notesBlockMarker, notesRichTextPreviewClass } from "$lib/notes/blocks/editor-ui";
  import { createCalloutPayload, DEFAULT_COLOR } from "$lib/notes/blocks/payloads";
  import { sectionNotesSlashCommandItems } from "$lib/notes/editor/slash-commands";
  import type { NotesBlockType } from "$lib/notes/types";
  import { BUILD_PLATFORM_PROFILE } from "$lib/platform";
  import { getPreferences } from "$lib/stores/preferences.svelte";
  import { cn } from "$lib/utils";
  import { anchoredPanelStyle } from "$lib/utils/anchored-panel";
  import { scrollEdgeFadeAction } from "$lib/utils/scroll-edge-fade";
  import { pickSelectPopoverGeometry } from "$lib/utils/select-popover-position";
  import { welcomeCursorFrame, type WelcomeCursorPoint } from "$lib/vault/welcome-cursor";
  import { WELCOME_NO_HOVER, welcomeHoverAt, welcomeHoverSame } from "$lib/vault/welcome-hover";
  import {
    NOTES_CAST,
    NOTES_CURSOR,
    NOTES_HEADING_TYPE,
    NOTES_NEW_CHARACTER,
    NOTES_NEW_ROLE,
    NOTES_ROLE_COLORS,
    NOTES_ROLE_ORDER,
    notesBoardActive,
    notesBoardScroll,
    notesBoardShown,
    notesCalloutAdded,
    notesHeadingAdded,
    notesHeadingCaretOn,
    notesHeadingText,
    notesLineCaretOn,
    notesLineText,
    notesNameCaretOn,
    notesNameDraft,
    notesNameFocused,
    notesNewRole,
    notesOptionHovered,
    notesPageScroll,
    notesParagraphCaretOn,
    notesParagraphShown,
    notesParagraphText,
    notesRoleHighlighted,
    notesRoleMenuOpen,
    notesRowAdded,
    notesSlashLineShown,
    notesSlashMenuOpen,
    notesSlashQuery,
    notesTouches,
    type NotesSceneCharacter,
    type NotesSceneRole,
    type NotesSceneRow,
  } from "$lib/vault/welcome-notes-scene";

  /**
   * One frame of the welcome scene on Notes, drawn with the app's own surfaces: on the Novel project's Characters page,
   * the Markdown shortcut for a heading turns an empty line into one, a paragraph is typed below it, a slash on the next
   * line opens the slash menu, which narrows to Callout as a query is typed, and the accepted callout is filled in. The
   * page then scrolls to its Cast database, where a row is added to the table, named, and given a role from its cell, and
   * the database switches to its Board view, which scrolls sideways to the new row's column. Every style is a function of
   * `elapsed`, with no CSS transitions.
   */
  let { elapsed, width, height }: { elapsed: number; width: number; height: number } = $props();

  const localization = getLocalization();
  const { t } = localization;
  const preferences = getPreferences();
  const mobileShell = BUILD_PLATFORM_PROFILE.shell === "mobile";

  /** The title bar on desktop, or the phone's top bar, tall enough for its context switcher. */
  const TOP_BAR_H = mobileShell ? 48 : 36;
  /** The page header, as `--toolbar-h` sizes it on each shell. */
  const HEADER_H = mobileShell ? 56 : 40;
  /** The title bar's end padding, as the title bar draws it. */
  const TITLE_BAR_END_PX = 6;
  /** The header's icons, as the breadcrumb and the page actions draw them. */
  const IDENTITY_ICON_SIZE = 14;
  const IDENTITY_ICON_STROKE_WIDTH = 1.5;
  const TOOL_ICON_STROKE_WIDTH = 1.75;
  const MOBILE_ICON_SIZE = 16;
  /** A phone header icon button, as the page header draws it. */
  const MOBILE_CHROME_BUTTON_CLASS = "mobile-chrome-button flex w-(--mobile-chrome-column) shrink-0 items-center justify-center text-foreground";
  /** A page action in the desktop header. */
  const PAGE_ACTION_CLASS = "flex h-7 min-w-7 shrink-0 items-center justify-center rounded-md px-1.5 text-foreground";

  /** The editor's side padding, as the page pads itself on wide and narrow panes. */
  const EDITOR_WIDE_W = 640;
  const EDITOR_PAD_WIDE = 32;
  const EDITOR_PAD_NARROW = 16;
  /** The table's tracks: the selection and expand gutter, then Name, Role, and the add-property track, as the table sizes them. */
  const TABLE_GUTTER = "calc(1.5rem + 1.75rem)";
  const TABLE_TEMPLATE = "1.5rem 1.75rem minmax(7rem, 220px) minmax(7rem, 180px) minmax(2.25rem, 1fr)";
  /** How far the table hangs into the page margin, at most the gutter's width, as the table does. */
  const TABLE_HANG_MAX_PX = 56;
  /** Room kept below the database once the page scrolls to it, and above its title at most. */
  const SCROLL_PAD_PX = 16;
  /** The slash menu's preferred height, in rem, as the editor caps it. */
  const SLASH_MAX_HEIGHT_REM = 22;
  /** A zero-width character that gives the slash line's anchor the height of a line of text. */
  const ZERO_WIDTH_SPACE = "​";
  /** The callout the slash line turns into, with the color and icon a new callout starts with. */
  const CALLOUT = createCalloutPayload("");
  const CALLOUT_LAYERS: readonly NotesCalloutLayer[] = [{ id: "callout", color: CALLOUT.color ?? DEFAULT_COLOR, first: true, last: true }];

  /** A block the page starts with: its type and the key of its text. */
  interface NotesSceneBlock {
    type: NotesBlockType;
    text: "quote" | "threads" | "bullet";
  }

  /** The blocks the page starts with, above the empty line the scene writes into. */
  const PAGE_BLOCKS: readonly NotesSceneBlock[] = [
    { type: "quote", text: "quote" },
    { type: "heading_2", text: "threads" },
    { type: "bulleted_list_item", text: "bullet" },
  ];

  const canvasW = $derived(width - 2 * BEZEL);
  const canvasH = $derived(height - 2 * BEZEL);
  const editorPad = $derived(canvasW >= EDITOR_WIDE_W ? EDITOR_PAD_WIDE : EDITOR_PAD_NARROW);
  const tableHang = $derived(Math.min(editorPad, TABLE_HANG_MAX_PX));

  const slashQuery = $derived(t("vaultSetup.notes.slashQuery"));
  const headingAdded = $derived(notesHeadingAdded(elapsed));
  const headingText = $derived(notesHeadingText(elapsed, t("vaultSetup.notes.heading")));
  const headingCaretOn = $derived(notesHeadingCaretOn(elapsed));
  const paragraphShown = $derived(notesParagraphShown(elapsed));
  const paragraphText = $derived(notesParagraphText(elapsed, t("vaultSetup.notes.paragraph")));
  const paragraphCaretOn = $derived(notesParagraphCaretOn(elapsed));
  const slashLineShown = $derived(notesSlashLineShown(elapsed));
  const lineText = $derived(notesLineText(elapsed, slashQuery, t("vaultSetup.notes.callout")));
  const lineCaretOn = $derived(notesLineCaretOn(elapsed));
  const calloutAdded = $derived(notesCalloutAdded(elapsed));
  const slashOpen = $derived(notesSlashMenuOpen(elapsed));
  const typedQuery = $derived(notesSlashQuery(elapsed, slashQuery));
  /** The slash menu's groups for the query typed so far, as a fresh vault lists them with no recent blocks yet. */
  const slashGroups = $derived(
    notesSlashMenuGroups(sectionNotesSlashCommandItems(notesSlashMenuCatalog(true, t), typedQuery, []), t).filter((group) => group.items.length > 0),
  );
  /** The menu highlights its first item, as it does while nothing else is under the pointer. */
  const slashActiveKey = $derived(slashGroups[0]?.items[0]?.key);

  const scroll = $derived(notesPageScroll(elapsed));
  const rowAdded = $derived(notesRowAdded(elapsed));
  const nameFocused = $derived(notesNameFocused(elapsed));
  const newName = $derived(t(`vaultSetup.notes.characters.${NOTES_NEW_CHARACTER}`));
  const nameDraft = $derived(notesNameDraft(elapsed, newName));
  const caretOn = $derived(notesNameCaretOn(elapsed));
  const menuOpen = $derived(notesRoleMenuOpen(elapsed));
  const newRole = $derived(notesNewRole(elapsed));
  const boardActive = $derived(notesBoardActive(elapsed));
  const boardShown = $derived(notesBoardShown(elapsed));
  const boardScroll = $derived(notesBoardScroll(elapsed));
  const touches = $derived(notesTouches(elapsed));

  /** The database's rows: the cast it starts with, then the added row with the role it has so far. */
  const rows = $derived<readonly NotesSceneRow[]>(rowAdded ? [...NOTES_CAST, { key: NOTES_NEW_CHARACTER, role: newRole }] : NOTES_CAST);
  /** Each role's cards on the board, in row order. */
  const columns = $derived(NOTES_ROLE_ORDER.map((role) => ({ role, cards: rows.filter((row) => row.role === role) })));

  /** Whether the desktop pointer has left the canvas, where it hovers the named parts under it. A phone has touches and no hover. */
  const cursorAway = $derived(welcomeCursorFrame(NOTES_CURSOR, elapsed).away);
  let cursorPoint = $state<WelcomeCursorPoint | null>(null);
  let hover = $state<readonly string[]>(WELCOME_NO_HOVER);

  let canvas = $state<HTMLDivElement>();
  let pageViewport = $state<HTMLDivElement>();
  let pageContent = $state<HTMLDivElement>();
  let database = $state<HTMLDivElement>();
  let roleCell = $state<HTMLDivElement>();
  let roleMenu = $state<HTMLDivElement>();
  let slashAnchor = $state<HTMLSpanElement>();
  let slashMenu = $state<HTMLDivElement>();
  let slashScroll = $state<HTMLDivElement>();
  let boardViewport = $state<HTMLDivElement>();
  let boardTrack = $state<HTMLDivElement>();
  let boardColumns = $state<Partial<Record<NotesSceneRole, HTMLDivElement>>>({});

  /** A box in the logical pixels of the element it is measured against. */
  interface Box {
    top: number;
    left: number;
    width: number;
    height: number;
  }

  /** Bumped whenever the canvas or the page resizes, or fonts finish loading, so the layout is measured again. */
  let layoutTick = $state(0);
  /** The page's scroll range down to the database, measured unscrolled. */
  let pageLayout = $state<{ viewport: number; top: number; bottom: number } | null>(null);
  /** The role cell on the canvas and the role menu's size, measured as the menu opens. */
  let menuLayout = $state<{ cell: Box; width: number; height: number } | null>(null);
  /**
   * The slash line's start on the canvas, the slash menu's chrome and list heights, and the root font size its height cap
   * is in, measured as its contents change.
   */
  let slashLayout = $state<{ trigger: Box; chrome: number; list: number; rem: number } | null>(null);
  /** The board's visible width, the width of all its columns, and the new row's column within them, measured unscrolled. */
  let boardLayout = $state<{ viewport: number; content: number; left: number; right: number } | null>(null);

  $effect(() => {
    const bump = () => {
      layoutTick += 1;
    };
    const observed = [canvas, pageContent].filter((element): element is HTMLDivElement => element !== undefined);
    const observer = typeof ResizeObserver === "undefined" ? null : new ResizeObserver(bump);
    for (const element of observed) observer?.observe(element);
    const fonts = typeof document === "undefined" ? undefined : document.fonts;
    fonts?.addEventListener("loadingdone", bump);
    return () => {
      observer?.disconnect();
      fonts?.removeEventListener("loadingdone", bump);
    };
  });

  /** Measure the page and its menus once they lay themselves out for the canvas, the language, and the typed query. */
  $effect(() => {
    void layoutTick;
    void canvasW;
    void canvasH;
    void localization.locale;
    void menuOpen;
    void typedQuery;
    if (pageViewport && pageContent && database) {
      const box = relBox(database, pageContent);
      pageLayout = { viewport: pageViewport.clientHeight, top: box.top, bottom: box.top + box.height };
    }
    if (canvas && roleCell && roleMenu) {
      menuLayout = { cell: relBox(roleCell, canvas), width: roleMenu.offsetWidth, height: roleMenu.offsetHeight };
    }
    const newColumn = boardColumns[NOTES_NEW_ROLE];
    if (boardViewport && boardTrack && newColumn) {
      boardLayout = {
        viewport: boardViewport.clientWidth,
        content: boardTrack.scrollWidth,
        left: newColumn.offsetLeft,
        right: newColumn.offsetLeft + newColumn.offsetWidth,
      };
    }
    if (canvas && slashAnchor && slashMenu && slashScroll) {
      slashLayout = {
        trigger: relBox(slashAnchor, canvas),
        chrome: slashMenu.offsetHeight - slashScroll.clientHeight,
        list: slashScroll.scrollHeight,
        rem: Number.parseFloat(getComputedStyle(document.documentElement).fontSize) || 16,
      };
    }
  });

  /** The slash menu's place beside the slash, as the editor places it, kept laid out while closed so its size is known. */
  const slashMenuStyle = $derived.by(() => {
    if (!slashLayout) return "visibility: hidden";
    const { trigger, chrome, list, rem } = slashLayout;
    const style = anchoredPanelStyle({
      triggerRect: { top: trigger.top, left: trigger.left, right: trigger.left + trigger.width, bottom: trigger.top + trigger.height },
      viewportWidth: canvasW,
      viewportHeight: canvasH,
      preferredWidth: FLOATING_WIDTH.lg,
      preferredMaxHeight: Math.min(SLASH_MAX_HEIGHT_REM * rem, list + chrome),
    });
    return slashOpen ? style : `${style}; visibility: hidden`;
  });

  /** How far the page has scrolled: enough to keep the database in view below, but never past its top. */
  const scrollPx = $derived.by(() => {
    if (!pageLayout) return 0;
    const target = Math.max(0, pageLayout.bottom + SCROLL_PAD_PX - pageLayout.viewport);
    return Math.min(target, Math.max(0, pageLayout.top - SCROLL_PAD_PX)) * scroll;
  });

  /**
   * How far the board has scrolled sideways: enough to show the new row's column in full, or to its start on a phone,
   * where a swipe settles on a column, but never past the last column.
   */
  const boardScrollPx = $derived.by(() => {
    if (!boardLayout) return 0;
    const { viewport, content, left, right } = boardLayout;
    const target = mobileShell ? left : Math.max(0, right - viewport);
    return Math.min(target, Math.max(0, content - viewport)) * boardScroll;
  });

  /** The open role menu's place, below its cell when it fits there and above it otherwise. */
  const menuPlace = $derived.by(() => {
    if (!menuLayout || !menuOpen) return null;
    const { cell } = menuLayout;
    return pickSelectPopoverGeometry({
      triggerRect: { ...cell, right: cell.left + cell.width, bottom: cell.top + cell.height },
      boundaryRect: { top: 0, left: 0, right: canvasW, bottom: canvasH, width: canvasW, height: canvasH },
      contentHeight: menuLayout.height,
      contentWidth: menuLayout.width,
    });
  });

  /** Follow the desktop pointer across the page as it moves and as the page scrolls or changes under it. */
  $effect(() => {
    void elapsed;
    void scrollPx;
    void layoutTick;
    const next = mobileShell || cursorAway || !canvas ? WELCOME_NO_HOVER : welcomeHoverAt(canvas, canvasW, cursorPoint);
    if (!welcomeHoverSame(next, untrack(() => hover))) hover = next;
  });

  /** Whether the part named `name` is under the desktop pointer. */
  function hovered(name: string): boolean {
    return hover.includes(name);
  }

  /** Whether the table holds its hover feedback back, as it does while a cell menu is open or a cell's text field is focused. */
  const tableHoverHeld = $derived(menuOpen || nameFocused);

  /** Whether a table row is under the desktop pointer. */
  function rowHovered(key: NotesSceneCharacter): boolean {
    return hovered(`row:${key}`);
  }

  /** The row highlight while the row is hovered and the table does not hold hover feedback back. */
  function rowStyle(key: NotesSceneCharacter): string | undefined {
    return !tableHoverHeld && rowHovered(key) ? "--collection-row-hover: var(--collection-row-highlight);" : undefined;
  }

  /**
   * The background of a table cell: highlighted under the pointer while the table does not hold hover feedback back, and
   * for the new row's title while it is focused and its role cell while its menu is open or from its touch on a phone.
   */
  function cellBackground(name: string, cell: "name" | "role" | null): string {
    const highlighted =
      (cell === "name" && nameFocused) ||
      (mobileShell ? cell === "role" && notesRoleHighlighted(elapsed) : (cell === "role" && menuOpen) || (!tableHoverHeld && hovered(name)));
    return `background-image: ${highlighted ? "linear-gradient(var(--collection-highlight), var(--collection-highlight))" : "none"}`;
  }

  /** A role's name, or the empty cell's label. */
  function roleName(role: NotesSceneRole | null): string {
    return role === null ? t("notes.databaseTableEmptyCell") : t(`vaultSetup.notes.roles.${role}`);
  }

  function characterName(key: NotesSceneCharacter): string {
    return t(`vaultSetup.notes.characters.${key}`);
  }

  /** Whether the Ally option shows as hovered: under the pointer on desktop, or just before its touch lands on a phone. */
  function optionHovered(role: NotesSceneRole | null): boolean {
    if (mobileShell) return role === NOTES_NEW_ROLE && notesOptionHovered(elapsed);
    return hovered(`option:${role ?? "empty"}`);
  }

  /**
   * An element's place within `ancestor`, in the ancestor's logical pixels, so the scene's scale and any transform the two
   * share do not count.
   */
  function relBox(element: HTMLElement, ancestor: HTMLElement): Box {
    const rect = element.getBoundingClientRect();
    const origin = ancestor.getBoundingClientRect();
    const scale = ancestor.offsetWidth > 0 && origin.width > 0 ? origin.width / ancestor.offsetWidth : 1;
    return {
      top: (rect.top - origin.top) / scale,
      left: (rect.left - origin.left) / scale,
      width: rect.width / scale,
      height: rect.height / scale,
    };
  }
</script>

<!-- A select option's colored dot, as the database draws it. -->
{#snippet dot(role: NotesSceneRole)}
  <span class="size-2 shrink-0 rounded-full" style={`${notesBlockColorStyle(NOTES_ROLE_COLORS[role])}; background-color: var(--notes-block-color);`}></span>
{/snippet}

<!-- A role as the board's column headers draw it. -->
{#snippet badge(role: NotesSceneRole)}
  <span
    class="inline-flex max-w-full min-w-0 items-center gap-1.5 rounded-full px-2 py-0.5 text-[0.733333rem] font-medium"
    style={`${notesBlockColorStyle(NOTES_ROLE_COLORS[role])}; background-color: color-mix(in srgb, var(--notes-block-color) 15%, var(--card)); color: var(--foreground);`}
  >
    <span class="size-2 shrink-0 rounded-full bg-(--notes-block-color)"></span>
    <span class="truncate">{roleName(role)}</span>
  </span>
{/snippet}

<!-- A board card, as the board draws it. -->
{#snippet card(row: NotesSceneRow)}
  {@const cardHovered = hovered(`card:${row.key}`)}
  <div class={cn("collection-card relative min-w-0 rounded-lg border bg-card text-card-foreground shadow-sm", cardHovered ? "border-foreground/25" : "border-border")} data-scene-hover={`card:${row.key}`}>
    <div class="p-3">
      <div class="flex min-w-0 items-start gap-1.5">
        {#if !mobileShell}
          <span
            data-scene-hover={`grip:${row.key}`}
            class={cn("mt-1 flex size-6 shrink-0 items-center justify-center rounded text-muted-foreground", hovered(`grip:${row.key}`) && "bg-accent")}
          >
            <GripVertical class="size-3.5" />
          </span>
        {/if}
        <span class={cn("flex min-h-8 min-w-0 flex-1 items-center rounded-sm text-left text-[0.866667rem] font-medium", cardHovered && "text-primary")}>
          <span class="line-clamp-3">{characterName(row.key)}</span>
        </span>
      </div>
    </div>
  </div>
{/snippet}

<div class="rounded-3xl bg-foreground/6 p-1.5">
  <div
    bind:this={canvas}
    class="relative flex flex-col overflow-hidden rounded-2xl bg-(--cal-bg) text-foreground"
    style:contain="layout"
    style:width={`${canvasW}px`}
    style:height={`${canvasH}px`}
  >
    {#if mobileShell}
      <!-- The phone's top bar, with the Novel project's context switcher. -->
      <div class="flex shrink-0 items-center gap-2 bg-background px-1.5" style:height={`${TOP_BAR_H}px`}>
        <span class="flex h-9 min-w-0 items-center gap-2 rounded-full bg-accent/55 pr-2.5 pl-2 text-identity font-medium">
          <span class="grid size-5 shrink-0 place-items-center"><Feather size={IDENTITY_ICON_SIZE} strokeWidth={IDENTITY_ICON_STROKE_WIDTH} /></span>
          <span class="truncate">{t("vaultSetup.projects.project")}</span>
          <ChevronDown size={16} class="shrink-0 text-muted-foreground" />
        </span>
      </div>
    {:else}
      <!-- The title bar's Music. -->
      <div class="flex shrink-0 items-center justify-end gap-0.5 bg-sidebar" style:height={`${TOP_BAR_H}px`} style:padding-right={`${TITLE_BAR_END_PX}px`}>
        <WelcomeTitleIcon icon={Music} />
      </div>
    {/if}

    <div class="relative flex min-h-0 flex-1 flex-col bg-(--cal-bg)">
      {#if mobileShell}
        <div class="flex shrink-0 items-center gap-0 px-(--mobile-chrome-inset)" style:height={`${HEADER_H}px`}>
          <div class="flex h-12 min-w-0 flex-1 items-center">
            <span class={MOBILE_CHROME_BUTTON_CLASS}><PanelLeft size={MOBILE_ICON_SIZE} strokeWidth={TOOL_ICON_STROKE_WIDTH} /></span>
            <span class="mobile-chrome-button flex min-w-0 items-center justify-start gap-1.5 px-1.5 text-foreground">
              <FileText size={MOBILE_ICON_SIZE} strokeWidth={TOOL_ICON_STROKE_WIDTH} class="shrink-0" />
              <span class="min-w-0 truncate text-xs font-medium">{t("vaultSetup.notes.page")}</span>
            </span>
          </div>
          <span class={MOBILE_CHROME_BUTTON_CLASS}><Star size={MOBILE_ICON_SIZE} strokeWidth={TOOL_ICON_STROKE_WIDTH} /></span>
          <span class={MOBILE_CHROME_BUTTON_CLASS}><Ellipsis size={MOBILE_ICON_SIZE} strokeWidth={TOOL_ICON_STROKE_WIDTH} /></span>
        </div>
      {:else}
        <div class="flex shrink-0 items-center gap-1 px-3" style:height={`${HEADER_H}px`}>
          <span class="flex h-7 min-w-0 items-center gap-0.5 overflow-hidden text-identity font-medium">
            <span class="flex h-7 min-w-0 items-center gap-1.5 rounded-md px-1.5">
              <Heart size={IDENTITY_ICON_SIZE} strokeWidth={IDENTITY_ICON_STROKE_WIDTH} class="shrink-0" />
              <span class="truncate">{t("vaultSetup.film.personalGroup")}</span>
            </span>
            <span class="shrink-0 px-0.5 font-semibold text-muted-foreground">/</span>
            <span class="flex h-7 min-w-0 items-center gap-1.5 rounded-md px-1.5">
              <Feather size={IDENTITY_ICON_SIZE} strokeWidth={IDENTITY_ICON_STROKE_WIDTH} class="shrink-0" />
              <span class="truncate">{t("vaultSetup.projects.project")}</span>
            </span>
            <span class="shrink-0 px-0.5 font-semibold text-muted-foreground">/</span>
            <span class="flex h-7 min-w-0 items-center gap-1.5 rounded-md px-1.5">
              <FileText size={IDENTITY_ICON_SIZE} strokeWidth={IDENTITY_ICON_STROKE_WIDTH} class="shrink-0" />
              <span class="truncate">{t("vaultSetup.notes.page")}</span>
              <WorkspaceBreadcrumbTerminalIcon kind="chevron" class="shrink-0 text-muted-foreground" />
            </span>
          </span>
          <span class="flex-1"></span>
          <span class="flex shrink-0 items-center gap-1">
            <span class={PAGE_ACTION_CLASS}><Share2 class="size-4" strokeWidth={TOOL_ICON_STROKE_WIDTH} /></span>
            <span class={PAGE_ACTION_CLASS}><Star class="size-4" strokeWidth={TOOL_ICON_STROKE_WIDTH} /></span>
            <span class={PAGE_ACTION_CLASS}><Ellipsis class="size-4" strokeWidth={TOOL_ICON_STROKE_WIDTH} /></span>
          </span>
        </div>
      {/if}

      <!-- The Characters page, scrolled down to its Cast database. -->
      <div bind:this={pageViewport} data-cursor="notes-page" class="relative min-h-0 flex-1 overflow-hidden">
        <div bind:this={pageContent} style:transform={scrollPx > 0 ? `translateY(${-scrollPx}px)` : undefined}>
          <div class="mx-auto flex w-full max-w-208 flex-col pt-3 pb-12" style:padding-inline={`${editorPad}px`}>
            <div class="min-w-0 pb-5">
              <h1 class="notes-editor-page-title block w-full min-w-0 font-bold leading-[1.2] break-words text-foreground">{t("vaultSetup.notes.page")}</h1>
            </div>

            <!-- The blocks the page starts with, as the editor draws them while none is being edited. -->
            {#each PAGE_BLOCKS as block (block.text)}
              {@const marker = notesBlockMarker(block.type)}
              <div class="flex min-w-0 items-start gap-1 rounded-md py-0.5 pr-2">
                {#if marker}
                  <div class="notes-editor-body-text min-w-5 shrink-0 select-none whitespace-nowrap py-1 text-right leading-normal text-muted-foreground">{marker}</div>
                {/if}
                <div class="relative min-w-0 flex-1">
                  <div class={notesRichTextPreviewClass(block.type)}>{t(`vaultSetup.notes.${block.text}`)}</div>
                </div>
              </div>
            {/each}

            <!-- The empty line below them, where the heading shortcut is typed, which turns into a heading once its space is. -->
            <div class="flex min-w-0 items-start gap-1 rounded-md py-0.5 pr-2">
              <div class="relative min-w-0 flex-1" data-cursor="notes-line">
                <div class={notesRichTextPreviewClass(headingAdded ? NOTES_HEADING_TYPE : "paragraph")}>{ZERO_WIDTH_SPACE}{headingText}{#if headingCaretOn}<WelcomeCaret />{/if}</div>
                {#if mobileShell}<WelcomeTouch opacity={touches.line} />{/if}
              </div>
            </div>

            <!-- The paragraph Enter starts below the heading. -->
            {#if paragraphShown}
              <div class="flex min-w-0 items-start gap-1 rounded-md py-0.5 pr-2">
                <div class="relative min-w-0 flex-1">
                  <div class={notesRichTextPreviewClass("paragraph")}>{ZERO_WIDTH_SPACE}{paragraphText}{#if paragraphCaretOn}<WelcomeCaret />{/if}</div>
                </div>
              </div>
            {/if}

            <!-- The line Enter starts below the paragraph, where the slash is typed, which turns into a callout once Callout is accepted. -->
            {#if slashLineShown && calloutAdded}
              <NotesCalloutEmbeddedRow layers={CALLOUT_LAYERS}>
                {#snippet body()}
                  <div class="notes-callout-surface flex min-w-0 items-start rounded-md pr-2">
                    <span class="notes-callout-icon flex size-6 shrink-0 items-center justify-center rounded">
                      <NotesPageIcon icon={CALLOUT.icon} size={16} class="notes-callout-icon-glyph" />
                    </span>
                    <div class="relative min-w-0 flex-1">
                      <div class={notesRichTextPreviewClass("callout")}>{ZERO_WIDTH_SPACE}{lineText}{#if lineCaretOn}<WelcomeCaret />{/if}</div>
                    </div>
                  </div>
                {/snippet}
              </NotesCalloutEmbeddedRow>
            {:else if slashLineShown}
              <div class="flex min-w-0 items-start gap-1 rounded-md py-0.5 pr-2">
                <div class="relative min-w-0 flex-1">
                  <div class={notesRichTextPreviewClass("paragraph")}><span bind:this={slashAnchor}>{ZERO_WIDTH_SPACE}</span>{lineText}{#if lineCaretOn}<WelcomeCaret />{/if}</div>
                </div>
              </div>
            {/if}

            <div bind:this={database} class="my-5 space-y-1">
              <div class="flex items-center gap-2 px-1">
                <span class="flex min-h-10 min-w-0 items-center truncate text-[1.4rem] leading-tight font-semibold">{t("vaultSetup.notes.database")}</span>
              </div>

              <!-- The database toolbar: its views, its settings, and New. -->
              <div class="flex flex-wrap items-center gap-1 py-1">
                <span
                  data-scene-hover="tab:table"
                  class={cn(
                    "inline-flex h-8 items-center gap-1.5 rounded-md px-2 text-[0.866667rem] font-medium",
                    !boardActive ? "bg-accent text-foreground" : hovered("tab:table") ? "bg-accent/60 text-foreground" : "text-muted-foreground",
                  )}
                >
                  <Table2 class="size-4" strokeWidth={TOOL_ICON_STROKE_WIDTH} />{t("notes.databaseViewTable")}
                </span>
                <span
                  data-cursor="notes-board-tab"
                  data-scene-hover="tab:board"
                  class={cn(
                    "relative inline-flex h-8 items-center gap-1.5 rounded-md px-2 text-[0.866667rem] font-medium",
                    boardActive ? "bg-accent text-foreground" : hovered("tab:board") ? "bg-accent/60 text-foreground" : "text-muted-foreground",
                  )}
                >
                  <Columns3 class="size-4" strokeWidth={TOOL_ICON_STROKE_WIDTH} />{t("notes.databaseViewBoard")}
                  {#if mobileShell}<WelcomeTouch opacity={touches.board} />{/if}
                </span>
                <span class="ml-auto flex items-center gap-1">
                  <span class="flex size-8 items-center justify-center rounded-md text-muted-foreground"><SlidersHorizontal class="size-4" strokeWidth={TOOL_ICON_STROKE_WIDTH} /></span>
                  <span class="flex items-center rounded-md bg-primary text-primary-foreground">
                    <span class="flex min-h-8 items-center rounded-l-md px-3 text-[0.866667rem] font-medium">{t("notes.databaseNew")}</span>
                    <span class="h-5 w-px bg-primary-foreground/25"></span>
                    <span class="flex min-h-8 items-center rounded-r-md px-1.5"><ChevronDown size={14} strokeWidth={2} /></span>
                  </span>
                </span>
              </div>

              <!-- The Table and Board views share one place, so the block keeps the taller one's height as they switch. -->
              <div class="grid">
                <div
                  class="relative min-w-0 pt-2 [grid-area:1/1]"
                  style:margin-left={`${-tableHang}px`}
                  style:visibility={boardShown >= 1 ? "hidden" : undefined}
                  style:opacity={boardShown > 0 ? 1 - boardShown : undefined}
                >
                  <CollectionRow template={TABLE_TEMPLATE} gutter={TABLE_GUTTER} header dividerTop>
                    <div></div>
                    <div></div>
                    <CollectionCell class="gap-1.5">
                      {#if preferences.notesShowPropertyIcons}<CollectionPropertyIcon kind="title" class="size-3.5 shrink-0 text-muted-foreground" />{/if}
                      <span class="truncate">{t("vaultSetup.notes.name")}</span>
                    </CollectionCell>
                    <CollectionCell class="gap-1.5">
                      {#if preferences.notesShowPropertyIcons}<CollectionPropertyIcon kind="select" class="size-3.5 shrink-0 text-muted-foreground" />{/if}
                      <span class="truncate">{t("vaultSetup.notes.role")}</span>
                    </CollectionCell>
                    <div class="grid min-h-(--collection-table-row-height) min-w-0 place-items-center self-stretch text-muted-foreground">
                      <span class="grid size-9 place-items-center"><Plus size={14} strokeWidth={1.75} class="size-3.5" /></span>
                    </div>
                  </CollectionRow>

                  {#each rows as row (row.key)}
                    {@const added = row.key === NOTES_NEW_CHARACTER}
                    {@const editing = added && nameFocused}
                    <CollectionRow template={TABLE_TEMPLATE} gutter={TABLE_GUTTER} data-scene-hover={`row:${row.key}`} style={rowStyle(row.key)}>
                      <div class="flex h-full items-center justify-center">
                        {#if mobileShell || rowHovered(row.key)}<span class="grid size-4 shrink-0 place-items-center rounded-sm border border-foreground/25"></span>{/if}
                      </div>
                      <div></div>
                      <CollectionCell data-scene-hover={`cell:${row.key}:name`} style={cellBackground(`cell:${row.key}:name`, added ? "name" : null)}>
                        <span class="flex h-8 w-full min-w-0 items-center rounded-sm border border-transparent px-1">
                          {#if editing}
                            <span class="min-w-0 truncate text-foreground">{nameDraft}{#if caretOn}<WelcomeCaret />{/if}</span>
                          {:else}
                            <span class="truncate text-foreground">{characterName(row.key)}</span>
                          {/if}
                        </span>
                        {#if !editing && !mobileShell && rowHovered(row.key)}
                          <span class="absolute top-1/2 right-1 z-10 flex h-6 -translate-y-1/2 items-center gap-1 rounded-md border border-border bg-popover px-1.5 text-[0.75rem] text-popover-foreground">
                            <PanelRight class="size-3.5" aria-hidden="true" /><span>{t("collections.property.open")}</span>
                          </span>
                        {/if}
                      </CollectionCell>
                      <CollectionCell data-scene-hover={`cell:${row.key}:role`} style={cellBackground(`cell:${row.key}:role`, added ? "role" : null)}>
                        {#if added}<div bind:this={roleCell} data-cursor="notes-role" class="absolute inset-0"></div>{/if}
                        <span class="relative flex h-7 w-full min-w-0 items-center gap-2 rounded-md px-1.5">
                          <span class="flex min-w-0 flex-1 items-center gap-1.5">
                            {#if row.role}{@render dot(row.role)}{/if}
                            <span class="truncate text-foreground">{roleName(row.role)}</span>
                          </span>
                          <ChevronDown size={13} strokeWidth={2} class="shrink-0 text-muted-foreground" />
                        </span>
                        {#if added && mobileShell}<WelcomeTouch opacity={touches.role} left="2rem" />{/if}
                      </CollectionCell>
                      <div></div>
                    </CollectionRow>
                  {/each}

                  <!-- The table's New page row, as its quick add draws it closed. -->
                  <CollectionRow template={TABLE_TEMPLATE} gutter={TABLE_GUTTER} data-scene-hover="row:new" style={!tableHoverHeld && hovered("row:new") ? "--collection-row-hover: var(--collection-row-highlight);" : undefined}>
                    <span class={cn("col-span-full grid min-h-(--collection-table-row-height) grid-cols-subgrid items-center text-left text-collection", !tableHoverHeld && hovered("row:new") ? "text-foreground" : "text-muted-foreground")}>
                      <span class="flex min-w-0 items-center gap-2 px-2" style:grid-column="3 / -1">
                        <Plus class="size-4 shrink-0" />
                        <span class="relative min-w-0" data-cursor="notes-new-row">
                          <span class="block truncate">{t("notes.databaseNewPage")}</span>
                          {#if mobileShell}<WelcomeTouch opacity={touches.newRow} />{/if}
                        </span>
                      </span>
                    </span>
                  </CollectionRow>
                  <!-- Room for the row the scene adds, kept from the start so the page scrolls to where the table ends up. -->
                  {#if !rowAdded}<div class="min-h-(--collection-table-row-height)" aria-hidden="true"></div>{/if}
                </div>

                <!-- The Board view, with a column for each role, scrolled sideways as the board scrolls past the columns that do not fit. -->
                <div
                  bind:this={boardViewport}
                  class="min-w-0 overflow-hidden @container [grid-area:1/1]"
                  style:visibility={boardShown <= 0 ? "hidden" : undefined}
                  style:opacity={boardShown > 0 && boardShown < 1 ? boardShown : undefined}
                >
                  <div bind:this={boardTrack} class="relative flex min-w-0 gap-3 py-2" style:transform={boardScrollPx > 0 ? `translateX(${-boardScrollPx}px)` : undefined}>
                  {#each columns as column (column.role)}
                    <div
                      bind:this={boardColumns[column.role]}
                      class="scene-board-column flex min-h-56 shrink-0 flex-col rounded-lg border border-transparent bg-muted/20 p-1"
                    >
                      <div class="flex min-h-10 min-w-0 items-center gap-2 px-2">
                        <span class="min-w-0 flex-1">{@render badge(column.role)}</span>
                        <span class="text-[0.8rem] text-muted-foreground tabular-nums">{formatNumber(localization.locale, column.cards.length)}</span>
                        <span class="inline-flex size-8 shrink-0 items-center justify-center rounded text-muted-foreground"><Ellipsis class="size-3.5" strokeWidth={1.75} /></span>
                      </div>
                      <div class="flex min-h-0 flex-1 flex-col gap-2 px-1 pb-1">
                        {#each column.cards as entry (entry.key)}
                          {@render card(entry)}
                        {:else}
                          <p class="rounded-md border border-dashed border-border px-3 py-5 text-[0.8rem] text-muted-foreground">{t("notes.databaseRowsEmpty")}</p>
                        {/each}
                      </div>
                      <div class="mt-1 shrink-0">
                        <span
                          data-scene-hover={`add-card:${column.role}`}
                          class={cn(
                            "flex min-h-(--collection-table-row-height) w-full min-w-0 items-center gap-2 rounded-md px-2 text-left text-collection",
                            hovered(`add-card:${column.role}`) ? "bg-accent/40 text-foreground" : "text-muted-foreground",
                          )}
                        >
                          <Plus class="size-4 shrink-0" /><span class="truncate">{t("notes.databaseBoardAddCard")}</span>
                        </span>
                      </div>
                    </div>
                  {/each}
                  </div>
                </div>
              </div>
            </div>
          </div>
        </div>
      </div>

      <!-- The new row's role options, as its select lists them. It stays laid out while hidden so its size is known. -->
      <div
        bind:this={roleMenu}
        data-scene-cover
        class="surface-floating surface-floating-body absolute z-40 w-44"
        style:left={`${menuPlace?.left ?? 0}px`}
        style:top={`${(menuPlace?.top ?? 0) - TOP_BAR_H}px`}
        style:min-width={menuPlace ? `${menuPlace.minWidth}px` : undefined}
        style:visibility={menuPlace ? undefined : "hidden"}
      >
        {#each [null, ...NOTES_ROLE_ORDER] as role (role ?? "empty")}
          {@const picked = role === NOTES_NEW_ROLE}
          <span
            data-cursor={picked ? "notes-role-option" : undefined}
            data-scene-hover={`option:${role ?? "empty"}`}
            class={cn("menu-item relative justify-between gap-3", optionHovered(role) && "bg-accent")}
          >
            <span class="flex min-w-0 flex-1 items-center gap-1.5">
              {#if role}{@render dot(role)}{/if}
              <span class="truncate">{roleName(role)}</span>
            </span>
            {#if role === newRole}<Check size={12} strokeWidth={2.5} class="shrink-0" />{/if}
            {#if picked && mobileShell}<WelcomeTouch opacity={touches.roleOption} left="2.5rem" />{/if}
          </span>
        {/each}
      </div>
    </div>

    <!--
      The slash menu, as the editor lists its commands for the query after the slash. The canvas contains its layout, so
      the fixed place the editor gives the menu lands on the canvas rather than the window.
    -->
    <div bind:this={slashMenu} data-scene-cover class="surface-floating fixed top-0 left-0 z-50 flex w-floating-lg flex-col overflow-hidden" style={slashMenuStyle}>
      <div class="flex shrink-0 items-center gap-2 border-b border-border px-3 py-2 text-muted-foreground">
        <Search class="size-4 shrink-0" />
        <span class="truncate">{typedQuery || t("notes.slashSearch")}</span>
      </div>
      <div bind:this={slashScroll} class="min-h-0 overflow-y-auto overscroll-contain" use:scrollEdgeFadeAction>
        <div class="surface-floating-body">
          {#each slashGroups as group (group.label)}
            <div class="menu-label">{group.label}</div>
            {#each group.items as item (item.key)}
              {@const Icon = notesSlashCommandIcon(item.command)}
              {@const hint = notesSlashCommandHint(item.command)}
              {@const picked = item.command.kind === "block" && item.command.blockType === "callout"}
              <span class="menu-item relative" data-highlighted={item.key === slashActiveKey ? "" : undefined}>
                {#if item.command.kind === "color"}
                  <span class="notes-color-swatch" style={notesBlockColorSwatchStyle(item.command.color)}>A</span>
                {:else if Icon}
                  <Icon class="size-4 shrink-0" />
                {/if}
                <span class="min-w-0 flex-1 truncate">{notesSlashCommandLabel(item.command, t)}</span>
                {#if hint}<span class="text-panel-detail text-muted-foreground">{hint}</span>{/if}
                {#if picked && mobileShell}<WelcomeTouch opacity={touches.pick} left="2rem" />{/if}
              </span>
            {/each}
          {/each}
        </div>
      </div>
    </div>

    {#if !mobileShell}
      <WelcomeCursor script={NOTES_CURSOR} {elapsed} width={canvasW} bind:position={cursorPoint} />
    {/if}
  </div>
</div>

<style>
  /* A board column keeps its width, as the board sizes it, so columns that do not fit scroll rather than shrink. */
  .scene-board-column {
    width: min(18rem, calc(100cqw - 0.5rem));
    max-height: 36rem;
  }

  /* The callout's row inside its background, as the editor draws a callout block. */
  .notes-callout-surface {
    min-height: 2.25rem;
    padding-block: 0.2rem;
    gap: var(--notes-callout-inset, 0.75rem);
  }

  .notes-callout-surface :global(.notes-callout-icon) {
    margin-top: calc(0.75rem * var(--font-scale) - 0.5rem);
  }

  .notes-callout-surface :global(.notes-callout-icon-glyph) {
    transform: scale(var(--font-scale));
  }

  /* The slash menu's color swatch, as the editor's slash menu draws it. */
  .notes-color-swatch {
    display: inline-flex;
    width: 1rem;
    height: 1rem;
    flex-shrink: 0;
    align-items: center;
    justify-content: center;
    border: 1px solid var(--notes-color-swatch-border);
    border-radius: 0.25rem;
    background: var(--notes-color-swatch-bg);
    color: var(--notes-color-swatch-fg);
    font-size: calc(0.65rem * var(--type-scale));
    font-weight: 600;
    line-height: 1;
  }
</style>
