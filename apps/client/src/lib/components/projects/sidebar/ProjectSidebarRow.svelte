<script lang="ts">
  import { tick } from "svelte";
  import Archive from "@lucide/svelte/icons/archive";
  import ArchiveRestore from "@lucide/svelte/icons/archive-restore";
  import ChevronDown from "@lucide/svelte/icons/chevron-down";
  import ChevronRight from "@lucide/svelte/icons/chevron-right";
  import EllipsisVertical from "@lucide/svelte/icons/ellipsis-vertical";
  import Eye from "@lucide/svelte/icons/eye";
  import EyeOff from "@lucide/svelte/icons/eye-off";
  import FolderInput from "@lucide/svelte/icons/folder-input";
  import Pencil from "@lucide/svelte/icons/pencil";
  import Settings2 from "@lucide/svelte/icons/settings-2";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import { COMPACT_IDENTITY_EMOJI_SCALE, COMPACT_IDENTITY_ICON_SIZE, COMPACT_IDENTITY_ICON_STROKE_WIDTH } from "$lib/icon-sizing";
  import { normalizeProjectName } from "$lib/projects/text";
  import type { ProjectSidebarActions } from "$lib/projects/sidebar";
  import type { Project, ProjectGroup, ProjectLifecycleStatus } from "$lib/projects/types";
  import { anchoredPopover } from "$lib/utils/anchored-popover";
  import { portal } from "$lib/utils/portal";
  import { scrollEdgeFadeAction } from "$lib/utils/scroll-edge-fade";
  import ProjectIcon from "../ProjectIcon.svelte";

  /**
   * One project in the Projects sidebar, with a row menu to rename it, move it to another group, hide or archive it, and
   * open its settings. Built-in projects show rename and move as unavailable.
   */
  let {
    project,
    selected,
    groupName = null,
    actions,
    moveGroups,
    onSelect,
    onRename,
    onMoveToGroup,
    onSetStatus,
    onOpenSettings = undefined,
  }: {
    project: Project;
    selected: boolean;
    /** Group shown after the name, such as in search results. */
    groupName?: string | null;
    actions: ProjectSidebarActions;
    /** Groups listed under Move to group, in sidebar order. */
    moveGroups: readonly ProjectGroup[];
    onSelect: () => void;
    onRename: (name: string) => Promise<void>;
    onMoveToGroup: (groupId: string) => Promise<void>;
    onSetStatus: (status: ProjectLifecycleStatus) => Promise<void>;
    /** Opens the project's settings; the item is left out where no settings panel exists. */
    onOpenSettings?: () => void;
  } = $props();

  const { t } = getLocalization();

  let menuOpen = $state(false);
  let moveOpen = $state(false);
  let editing = $state(false);
  let nameDraft = $state("");
  let pendingName = $state<string | null>(null);
  let actionButton = $state<HTMLButtonElement | null>(null);
  let menuElement = $state<HTMLDivElement | null>(null);
  let renameInput = $state<HTMLInputElement | null>(null);

  const displayName = $derived(pendingName ?? project.name);
  const lockedReason = $derived(actions.identityLocked ? t("projects.sidebar.identityLocked") : undefined);
  const moveUnavailableReason = $derived(lockedReason ?? (actions.canMoveToGroup ? undefined : t("projects.sidebar.noOtherGroup")));

  $effect(() => {
    if (pendingName !== null && project.name === pendingName) pendingName = null;
  });

  function openMenu(): void {
    if (editing) return;
    menuOpen = true;
    moveOpen = false;
    void tick().then(() => menuElement?.querySelector<HTMLButtonElement>("button")?.focus());
  }

  function closeMenu(): void {
    menuOpen = false;
    moveOpen = false;
  }

  function toggleMenu(): void {
    if (menuOpen) closeMenu();
    else openMenu();
  }

  function openContextMenu(event: MouseEvent): void {
    event.preventDefault();
    openMenu();
  }

  function closeMenuAndRestoreFocus(): void {
    closeMenu();
    actionButton?.focus();
  }

  /** Runs a menu action after closing the menu, and reports a failed write with its context. */
  function runAction(description: string, action: () => Promise<void>): void {
    closeMenu();
    void action().catch((error: unknown) => {
      console.error(`Failed to ${description} from the Projects sidebar`, error);
    });
  }

  function beginRename(): void {
    if (!actions.canRename) return;
    closeMenu();
    nameDraft = project.name;
    editing = true;
    void tick().then(() => {
      renameInput?.focus();
      renameInput?.select();
    });
  }

  function saveRename(): void {
    if (!editing) return;
    editing = false;
    const name = normalizeProjectName(nameDraft);
    if (!name || name === project.name) return;
    pendingName = name;
    void onRename(name).catch((error: unknown) => {
      pendingName = null;
      console.error("Failed to rename a project from the Projects sidebar", error);
    });
  }

  function cancelRename(): void {
    editing = false;
    void tick().then(() => actionButton?.focus());
  }

  function handleRenameKeydown(event: KeyboardEvent): void {
    if (event.key === "Enter") {
      event.preventDefault();
      saveRename();
      void tick().then(() => actionButton?.focus());
    } else if (event.key === "Escape") {
      event.preventDefault();
      event.stopPropagation();
      cancelRename();
    }
  }
</script>

<div
  class="explorer-row flex min-w-0 items-center"
  class:explorer-selected={selected}
  role="group"
  aria-label={displayName}
  data-project-id={project.id}
>
  {#if editing}
    <div class="flex min-w-0 flex-1 items-center gap-1.5 px-2">
      <ProjectIcon
        name={project.icon}
        size={COMPACT_IDENTITY_ICON_SIZE}
        strokeWidth={COMPACT_IDENTITY_ICON_STROKE_WIDTH}
        emojiScale={COMPACT_IDENTITY_EMOJI_SCALE}
        class="explorer-row-icon"
      />
      <input
        bind:this={renameInput}
        bind:value={nameDraft}
        class="explorer-row-label min-w-0 flex-1 bg-transparent text-inherit caret-primary outline-none placeholder:text-muted-foreground"
        data-app-shortcuts="ignore"
        aria-label={t("projects.sidebar.name")}
        placeholder={t("projects.sidebar.name")}
        onkeydown={handleRenameKeydown}
        onblur={saveRename}
      />
    </div>
  {:else}
    <button
      type="button"
      class="explorer-row-label flex min-w-0 flex-1 items-center gap-1.5 self-stretch px-2 text-left text-inherit"
      class:text-muted-foreground={project.status !== "active"}
      aria-current={selected ? "true" : undefined}
      onclick={onSelect}
      oncontextmenu={openContextMenu}
    >
      <ProjectIcon
        name={project.icon}
        size={COMPACT_IDENTITY_ICON_SIZE}
        strokeWidth={COMPACT_IDENTITY_ICON_STROKE_WIDTH}
        emojiScale={COMPACT_IDENTITY_EMOJI_SCALE}
        class="explorer-row-icon"
      />
      <span class="min-w-0 flex-1 truncate">{displayName}</span>
      {#if groupName}
        <span class="max-w-1/2 shrink-0 truncate text-muted-foreground">{groupName}</span>
      {/if}
    </button>
    <button
      bind:this={actionButton}
      type="button"
      class="explorer-icon explorer-row-action"
      aria-label={t("projects.sidebar.projectActions", displayName)}
      data-app-tooltip-disabled="true"
      aria-haspopup="menu"
      aria-expanded={menuOpen}
      onclick={toggleMenu}
    >
      <EllipsisVertical size={14} />
    </button>
  {/if}
</div>

{#if menuOpen}
  <div
    bind:this={menuElement}
    use:portal
    use:anchoredPopover={{ anchor: actionButton ?? undefined, onOutside: closeMenu }}
    class="surface-floating fixed z-90 flex w-floating-sm flex-col overflow-hidden"
    role="menu"
    tabindex="-1"
    aria-label={t("projects.sidebar.projectActions", displayName)}
    data-app-floating-surface
    onkeydown={(event) => {
      if (event.key !== "Escape") return;
      event.stopPropagation();
      closeMenuAndRestoreFocus();
    }}
  >
    <div class="surface-floating-body flex min-h-0 flex-1 flex-col overflow-y-auto overflow-x-hidden" use:scrollEdgeFadeAction>
      <button
        type="button"
        role="menuitem"
        class="menu-item"
        aria-disabled={actions.canRename ? undefined : "true"}
        data-app-tooltip={lockedReason}
        onclick={beginRename}
      >
        <Pencil />
        <span>{t("projects.sidebar.rename")}</span>
      </button>
      <button
        type="button"
        role="menuitem"
        class="menu-item"
        aria-disabled={actions.canMoveToGroup ? undefined : "true"}
        aria-expanded={actions.canMoveToGroup ? moveOpen : undefined}
        data-app-tooltip={moveUnavailableReason}
        onclick={() => {
          if (actions.canMoveToGroup) moveOpen = !moveOpen;
        }}
      >
        <FolderInput />
        <span class="min-w-0 flex-1 truncate">{t("projects.sidebar.moveToGroup")}</span>
        {#if actions.canMoveToGroup}
          {#if moveOpen}<ChevronDown />{:else}<ChevronRight />{/if}
        {/if}
      </button>
      {#if moveOpen}
        <div class="flex max-h-48 flex-col overflow-y-auto border-y border-border bg-muted/25 py-1" role="group" aria-label={t("projects.sidebar.moveToGroup")}>
          {#each moveGroups as group (group.id)}
            <button
              type="button"
              role="menuitem"
              class="menu-item"
              onclick={() => runAction("move a project to another group", () => onMoveToGroup(group.id))}
            >
              <ProjectIcon
                name={group.icon}
                size={COMPACT_IDENTITY_ICON_SIZE}
                strokeWidth={COMPACT_IDENTITY_ICON_STROKE_WIDTH}
                emojiScale={COMPACT_IDENTITY_EMOJI_SCALE}
              />
              <span class="min-w-0 flex-1 truncate">{group.name}</span>
            </button>
          {/each}
        </div>
      {/if}
      {#if onOpenSettings}
        <button
          type="button"
          role="menuitem"
          class="menu-item"
          onclick={() => {
            closeMenu();
            onOpenSettings?.();
          }}
        >
          <Settings2 />
          <span>{t("projects.sidebar.settings")}</span>
        </button>
      {/if}
      <div class="menu-separator" aria-hidden="true"></div>
      <button
        type="button"
        role="menuitem"
        class="menu-item"
        onclick={() => runAction("change a project's visibility", () => onSetStatus(actions.visibility.status))}
      >
        {#if actions.visibility.label === "show"}<Eye />{:else}<EyeOff />{/if}
        <span>{actions.visibility.label === "show" ? t("projects.sidebar.show") : t("projects.sidebar.hide")}</span>
      </button>
      <button
        type="button"
        role="menuitem"
        class="menu-item"
        onclick={() => runAction("archive or restore a project", () => onSetStatus(actions.archive.status))}
      >
        {#if actions.archive.label === "restore"}<ArchiveRestore />{:else}<Archive />{/if}
        <span>{actions.archive.label === "restore" ? t("projects.sidebar.restore") : t("projects.sidebar.archive")}</span>
      </button>
    </div>
  </div>
{/if}
