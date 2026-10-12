<script lang="ts">
  import { onMount, tick } from "svelte";
  import FolderX from "@lucide/svelte/icons/folder-x";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import { getProjects } from "$lib/stores/projects.svelte";
  import { cn } from "$lib/utils";
  import { portal } from "$lib/utils/portal";
  import ProjectIcon from "$lib/components/projects/ProjectIcon.svelte";
  import ProjectPickerPanels from "./ProjectPickerPanels.svelte";
  import ProjectPickerMobileDialog from "./ProjectPickerMobileDialog.svelte";
  import type { Project } from "$lib/projects/types";

  let {
    selectedProjectId = undefined,
    disabled = false,
    mobileLayout = false,
    onSelect,
  }: {
    selectedProjectId?: string;
    disabled?: boolean;
    mobileLayout?: boolean;
    onSelect: (projectId: string | undefined) => void;
  } = $props();

  const projects = getProjects();
  const { t } = getLocalization();
  const PROJECT_ICON_SIZE = 18;
  const PROJECT_EMOJI_SCALE = 0.82;
  const PROJECT_ICON_STROKE_WIDTH = 1.5;
  const EMPTY_ICON_STROKE_WIDTH = 1.6;
  const PANEL_MIN_WIDTH = 202;
  const PANEL_MAX_WIDTH = 259;
  const PANEL_RIGHT_NUDGE = 4;

  interface DropdownBounds {
    left: number;
    right: number;
    top: number;
    bottom: number;
  }

  let open = $state(false);
  let search = $state("");
  let rootEl: HTMLDivElement | undefined = $state();
  let triggerEl: HTMLButtonElement | undefined = $state();
  let dropdownStyle = $state("");
  let pickerMaxHeight = $state(0);
  let pickerHeight = $state(0);

  const selectedProject = $derived(projects.projectById(selectedProjectId));
  const selectedGroup = $derived(projects.groupById(selectedProject?.groupId));

  const pickerTitle = $derived.by(() => {
    if (!selectedProject) return t("calendar.eventPanel.projectPlaceholder");
    return selectedGroup ? `${selectedProject.name}, ${selectedGroup.name}` : selectedProject.name;
  });

  onMount(() => {
    void projects.ensureLoaded().catch((error) => {
      console.error("load projects failed", error);
    });

    const updateWhenOpen = () => {
      if (!open) return;
      updateDropdownGeometry();
    };
    window.addEventListener("resize", updateWhenOpen);
    window.addEventListener("scroll", updateWhenOpen, true);
    return () => {
      window.removeEventListener("resize", updateWhenOpen);
      window.removeEventListener("scroll", updateWhenOpen, true);
    };
  });

  function dropdownBounds(): DropdownBounds {
    const margin = 8;
    const panelRect = triggerEl?.closest(".event-panel-scroll")?.getBoundingClientRect();
    const viewportBounds = {
      left: margin,
      right: window.innerWidth - margin,
      top: margin,
      bottom: window.innerHeight - margin,
    };
    if (!panelRect) return viewportBounds;
    return {
      left: Math.max(panelRect.left, viewportBounds.left),
      right: Math.min(panelRect.right, viewportBounds.right),
      top: Math.max(panelRect.top, viewportBounds.top),
      bottom: Math.min(panelRect.bottom, viewportBounds.bottom),
    };
  }

  function dropdownAnchorRect(): DOMRect {
    const colorPickerEl = rootEl?.nextElementSibling;
    if (colorPickerEl instanceof HTMLElement) return colorPickerEl.getBoundingClientRect();
    return triggerEl?.getBoundingClientRect() ?? new DOMRect();
  }

  function updateDropdownGeometry(): void {
    if (mobileLayout) {
      dropdownStyle = "";
      pickerMaxHeight = 0;
      return;
    }
    if (!triggerEl) return;
    const anchorRect = dropdownAnchorRect();
    const bounds = dropdownBounds();
    const edge = 8;
    const gap = 4;
    const boundsWidth = Math.max(0, bounds.right - bounds.left);
    const width = Math.min(
      PANEL_MAX_WIDTH,
      Math.max(PANEL_MIN_WIDTH, boundsWidth),
    );
    const left = Math.min(
      Math.max(anchorRect.right + PANEL_RIGHT_NUDGE - width, bounds.left),
      bounds.right - width,
    );
    const maxHeight = Math.max(0, window.innerHeight - edge * 2);
    const height = Math.min(pickerHeight || maxHeight, maxHeight);
    const spaceBelow = window.innerHeight - edge - anchorRect.bottom - gap;
    const spaceAbove = anchorRect.top - edge - gap;
    const openAbove = spaceBelow < height && spaceAbove > spaceBelow;
    const preferredTop = openAbove
      ? anchorRect.top - gap - height
      : anchorRect.bottom + gap;
    const top = Math.min(
      Math.max(edge, preferredTop),
      Math.max(edge, window.innerHeight - edge - height),
    );

    pickerMaxHeight = maxHeight;
    dropdownStyle = [
      `left: ${Math.round(left)}px`,
      `top: ${Math.round(top)}px`,
      `width: ${Math.round(width)}px`,
    ].join("; ");
  }

  async function openDropdown(): Promise<void> {
    updateDropdownGeometry();
    open = true;
    await tick();
    updateDropdownGeometry();
  }

  function closeDropdown(): void {
    open = false;
  }

  function toggleDropdown(): void {
    if (open) closeDropdown();
    else void openDropdown();
  }

  async function selectProject(project: Project): Promise<void> {
    onSelect(project.id);
    closeDropdown();
  }

  async function clearSelection(): Promise<void> {
    onSelect(undefined);
    closeDropdown();
  }

  async function handleProjectCreated(): Promise<void> {
    onSelect(projects.selectedProjectId ?? undefined);
    closeDropdown();
  }

  $effect(() => {
    if (!open) return;
    const height = pickerHeight;
    void height;
    requestAnimationFrame(updateDropdownGeometry);
  });

</script>

<div bind:this={rootEl} class="relative flex items-center" data-app-shortcuts="ignore">
  <button
    bind:this={triggerEl}
    type="button"
    disabled={disabled}
    class={cn(
      "flex shrink-0 items-center justify-center text-event-panel-muted-text transition-colors hover:text-event-panel-input-text",
      "event-identity-trigger",
      mobileLayout ? "min-h-12 min-w-12 rounded-xl active:bg-accent" : "size-4.5 rounded-sm",
      disabled && "cursor-not-allowed opacity-60",
    )}
    title={pickerTitle}
    aria-label={pickerTitle}
    aria-haspopup="dialog"
    aria-expanded={open}
    data-app-tooltip-focus-disabled="true"
    onclick={() => {
      if (!disabled) toggleDropdown();
    }}
  >
    {#if selectedProject}
      <ProjectIcon
        name={selectedProject.icon}
        size={PROJECT_ICON_SIZE}
        strokeWidth={PROJECT_ICON_STROKE_WIDTH}
        emojiScale={PROJECT_EMOJI_SCALE}
      />
    {:else}
      <FolderX size={PROJECT_ICON_SIZE} strokeWidth={EMPTY_ICON_STROKE_WIDTH} />
    {/if}
  </button>

  {#if open}
    {#if mobileLayout}
      <ProjectPickerMobileDialog
        label={t("projects.navigator.pickerLabel")}
        closeLabel={t("projects.navigator.closePicker")}
        onClose={closeDropdown}
      >
        <ProjectPickerPanels
          selectedProjectId={selectedProjectId ?? null}
          bind:projectSearch={search}
          showClearProject
          closeOnProjectCreate
          onProjectSelected={selectProject}
          onProjectCreated={handleProjectCreated}
          onClearProject={clearSelection}
          mobileLayout
          onClose={closeDropdown}
        />
      </ProjectPickerMobileDialog>
    {:else}
      <!-- svelte-ignore a11y_click_events_have_key_events -->
      <!-- svelte-ignore a11y_no_static_element_interactions -->
      <div use:portal class="fixed inset-0 z-60" onclick={closeDropdown}></div>
      <div
        use:portal
        class="fixed z-61"
        style={dropdownStyle}
        role="dialog"
        tabindex="-1"
        aria-label={t("projects.navigator.pickerLabel")}
      >
        <ProjectPickerPanels
          selectedProjectId={selectedProjectId ?? null}
          bind:projectSearch={search}
          bind:panelHeight={pickerHeight}
          panelMaxHeight={pickerMaxHeight}
          showClearProject
          closeOnProjectCreate
          onProjectSelected={selectProject}
          onProjectCreated={handleProjectCreated}
          onClearProject={clearSelection}
        />
      </div>
    {/if}
  {/if}
</div>
