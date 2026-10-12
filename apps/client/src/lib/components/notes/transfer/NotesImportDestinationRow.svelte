<script lang="ts">
  import { onMount, tick } from "svelte";
  import ChevronDown from "@lucide/svelte/icons/chevron-down";
  import ProjectPickerPanels from "$lib/components/projects/pickers/ProjectPickerPanels.svelte";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import type { Project } from "$lib/projects/types";
  import { getProjects } from "$lib/stores/projects.svelte";
  import { cn } from "$lib/utils";
  import { portal } from "$lib/utils/portal";
  import NotesTransferFieldRow from "./NotesTransferFieldRow.svelte";

  let {
    projectId = $bindable<string | null>(null),
  }: {
    projectId?: string | null;
  } = $props();

  const { t } = getLocalization();
  const projects = getProjects();
  const panelWidthPx = 288;
  const panelGapPx = 4;
  const panelMarginPx = 8;
  const iconStrokeWidth = 1.6;
  let open = $state(false);
  let triggerEl = $state<HTMLButtonElement | undefined>();
  let panelHeight = $state(0);
  let panelMaxHeight = $state(0);
  let panelStyle = $state("");
  let projectSearch = $state("");

  const activeProjects = $derived.by(() =>
    [...projects.projects]
      .filter((project) => project.status === "active")
      .sort(compareProjects),
  );
  const selectedProject = $derived(projects.projectById(projectId ?? undefined));
  const selectedGroup = $derived(projects.groupById(selectedProject?.groupId));
  const selectedActiveProjectId = $derived(
    activeProjects.find((project) => project.id === projects.selectedProjectId)?.id ?? null,
  );
  const selectedLabel = $derived.by(() => {
    if (!selectedProject) return t("notes.importDestinationPlaceholder");
    return selectedGroup
      ? `${selectedGroup.name} / ${selectedProject.name}`
      : selectedProject.name;
  });

  $effect(() => {
    if (!projects.loaded || activeProjects.length === 0) return;
    if (projectId && activeProjects.some((project) => project.id === projectId)) return;
    projectId = selectedActiveProjectId ?? activeProjects[0]?.id ?? null;
  });

  onMount(() => {
    if (!projects.loaded) {
      void projects.ensureLoaded().catch(() => undefined);
    }

    const updateWhenOpen = () => {
      if (!open) return;
      updatePanelGeometry();
    };
    window.addEventListener("resize", updateWhenOpen);
    window.addEventListener("scroll", updateWhenOpen, true);
    return () => {
      window.removeEventListener("resize", updateWhenOpen);
      window.removeEventListener("scroll", updateWhenOpen, true);
    };
  });

  function compareProjects(left: Project, right: Project): number {
    const leftGroup = projects.groupById(left.groupId);
    const rightGroup = projects.groupById(right.groupId);
    return (leftGroup?.sortOrder ?? Number.MAX_SAFE_INTEGER)
      - (rightGroup?.sortOrder ?? Number.MAX_SAFE_INTEGER)
      || left.sortOrder - right.sortOrder
      || left.name.localeCompare(right.name);
  }

  function panelBounds(): { left: number; right: number; top: number; bottom: number } {
    const viewportBounds = {
      left: panelMarginPx,
      right: window.innerWidth - panelMarginPx,
      top: panelMarginPx,
      bottom: window.innerHeight - panelMarginPx,
    };
    const contentElement =
      triggerEl?.closest<HTMLElement>("[data-settings-content]")
      ?? triggerEl?.closest<HTMLElement>("[data-settings-modal-panel]");
    const contentRect = contentElement?.getBoundingClientRect();
    if (!contentRect) return viewportBounds;
    return {
      left: Math.max(contentRect.left, viewportBounds.left),
      right: Math.min(contentRect.right, viewportBounds.right),
      top: Math.max(contentRect.top, viewportBounds.top),
      bottom: Math.min(contentRect.bottom, viewportBounds.bottom),
    };
  }

  function updatePanelGeometry(): void {
    if (!triggerEl) return;
    const triggerRect = triggerEl.getBoundingClientRect();
    const bounds = panelBounds();
    const width = Math.min(panelWidthPx, Math.max(0, bounds.right - bounds.left));
    const left = Math.min(
      Math.max(triggerRect.left, bounds.left),
      Math.max(bounds.left, bounds.right - width),
    );
    const availableHeight = Math.max(0, bounds.bottom - bounds.top);
    const height = Math.min(panelHeight || availableHeight, availableHeight);
    const spaceBelow = bounds.bottom - triggerRect.bottom - panelGapPx;
    const spaceAbove = triggerRect.top - bounds.top - panelGapPx;
    const shouldOpenAbove = spaceBelow < height && spaceAbove > spaceBelow;
    const top = shouldOpenAbove
      ? Math.max(bounds.top, triggerRect.top - panelGapPx - height)
      : Math.min(triggerRect.bottom + panelGapPx, Math.max(bounds.top, bounds.bottom - height));

    panelMaxHeight = availableHeight;
    panelStyle = [
      `left: ${Math.round(left)}px`,
      `top: ${Math.round(top)}px`,
      `width: ${Math.round(width)}px`,
    ].join("; ");
  }

  async function openPanel(): Promise<void> {
    updatePanelGeometry();
    open = true;
    await tick();
    updatePanelGeometry();
  }

  function closePanel(): void {
    open = false;
    projectSearch = "";
  }

  function togglePanel(): void {
    if (open) {
      closePanel();
      return;
    }
    void openPanel();
  }

  async function selectProject(project: Project): Promise<void> {
    projectId = project.id;
    closePanel();
  }

  async function handleProjectCreated(): Promise<void> {
    projectId = projects.selectedProjectId;
    closePanel();
  }

  $effect(() => {
    if (!open) return;
    const height = panelHeight;
    void height;
    requestAnimationFrame(updatePanelGeometry);
  });
</script>

<svelte:window
  onkeydown={(event) => {
    if (!open || event.key !== "Escape") return;
    event.preventDefault();
    closePanel();
  }}
/>

<NotesTransferFieldRow
  label={t("notes.importDestinationLabel")}
  description={t("notes.importDestinationDescription")}
>
  <button
    bind:this={triggerEl}
    type="button"
    class="field flex w-72 max-w-full items-center justify-between gap-2 text-left text-panel font-medium text-foreground transition-colors hover:bg-accent max-[560px]:w-full"
    aria-label={t("notes.importDestinationLabel")}
    aria-haspopup="dialog"
    aria-expanded={open}
    onclick={togglePanel}
  >
    <span class="flex min-w-0 flex-1 items-center">
      <span class={cn("truncate", !selectedProject && "text-muted-foreground")}>{selectedLabel}</span>
    </span>
    <ChevronDown
      size={13}
      strokeWidth={iconStrokeWidth}
      class={cn("shrink-0 text-muted-foreground transition-transform", open && "rotate-180")}
    />
  </button>

  {#if open}
    <!-- svelte-ignore a11y_click_events_have_key_events -->
    <!-- svelte-ignore a11y_no_static_element_interactions -->
    <div use:portal class="fixed inset-0 z-70" onclick={closePanel}></div>
    <div
      use:portal
      class="fixed z-80"
      style={panelStyle}
      role="dialog"
      tabindex="-1"
      aria-label={t("projects.navigator.pickerLabel")}
    >
      <ProjectPickerPanels
        selectedProjectId={projectId}
        selectedGroupId={selectedGroup?.id ?? null}
        bind:projectSearch
        bind:panelHeight
        panelMaxHeight={panelMaxHeight}
        closeOnProjectCreate
        onProjectSelected={selectProject}
        onProjectCreated={handleProjectCreated}
      />
    </div>
  {/if}
</NotesTransferFieldRow>
