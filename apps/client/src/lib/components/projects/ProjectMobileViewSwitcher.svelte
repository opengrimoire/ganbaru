<script lang="ts">
  import { tick } from "svelte";
  import Check from "@lucide/svelte/icons/check";
  import ChevronDown from "@lucide/svelte/icons/chevron-down";
  import ChevronRight from "@lucide/svelte/icons/chevron-right";
  import Plus from "@lucide/svelte/icons/plus";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import { PROJECT_HOSTED_VIEW_TYPES, type ProjectView, type ProjectViewType } from "$lib/projects/types";
  import { projectViewAddOptions, projectViewsFor } from "$lib/projects/views";
  import { getMobileBackStack } from "$lib/stores/mobile-back-stack.svelte";
  import { getProjects } from "$lib/stores/projects.svelte";
  import { cn } from "$lib/utils";
  import { anchoredPopover } from "$lib/utils/anchored-popover";
  import { portal } from "$lib/utils/portal";
  import { scrollEdgeFadeAction } from "$lib/utils/scroll-edge-fade";
  import { PROJECT_VIEW_TYPE_ICONS, projectViewLabel, projectViewTypeLabel } from "./sidebar/project-view-presentation";

  /**
   * Phone toolbar switcher for the selected project's views: it names the open view, and its menu lists the views and
   * ends with an Add view row that expands the view types in place, so a new view is created and opened from the same
   * menu.
   */
  let {
    projectId,
    onSelectView,
  }: {
    projectId: string;
    /** Opens a view of the selected project. */
    onSelectView: (view: ProjectView) => void;
  } = $props();

  const projects = getProjects();
  const mobileBackStack = getMobileBackStack();
  const { t } = getLocalization();

  let menuOpen = $state(false);
  let addOpen = $state(false);
  let switcherButton = $state<HTMLButtonElement | null>(null);
  let addGroup = $state<HTMLDivElement | null>(null);

  const views = $derived(projects.viewsForProject(projectId));
  const activeView = $derived(views.find((view) => view.id === projects.activeViewId) ?? null);
  const activeLabel = $derived(activeView ? projectViewLabel(t, activeView) : t("projects.toolbar.views"));
  const ActiveIcon = $derived(activeView ? PROJECT_VIEW_TYPE_ICONS[activeView.viewType] : null);
  /**
   * Types a view can still be added for. Every view of the project counts, including hosted views this window hides, and
   * a hosted type the project already has is left out because touch has no tooltip to explain why it is unavailable.
   */
  const addableViewTypes = $derived(
    projectViewAddOptions(projectViewsFor(projects.views, projectId, PROJECT_HOSTED_VIEW_TYPES), projects.hostableViewTypes)
      .filter((option) => option.available)
      .map((option) => option.viewType),
  );

  $effect(() => {
    if (!menuOpen) return;
    return mobileBackStack.activate({ handle: closeMenu });
  });

  function closeMenu(): void {
    menuOpen = false;
    addOpen = false;
  }

  function toggleMenu(): void {
    if (menuOpen) closeMenu();
    else menuOpen = true;
  }

  /** Expands or collapses the view types, keeping them in sight when the menu scrolls. */
  function toggleAdd(): void {
    addOpen = !addOpen;
    if (!addOpen) return;
    void tick().then(() => addGroup?.scrollIntoView?.({ block: "nearest" }));
  }

  function selectView(view: ProjectView): void {
    closeMenu();
    if (view.id !== activeView?.id) onSelectView(view);
  }

  /** Adds a view of the chosen type with its type label as name, then opens it. */
  async function addView(viewType: ProjectViewType): Promise<void> {
    closeMenu();
    try {
      const viewId = await projects.addView(projectId, viewType);
      const view = projects.viewsForProject(projectId).find((entry) => entry.id === viewId);
      if (view) onSelectView(view);
    } catch (error: unknown) {
      console.error("Failed to add a project view from the phone toolbar", error);
    }
  }

  function handleMenuKeydown(event: KeyboardEvent): void {
    if (event.key !== "Escape") return;
    event.stopPropagation();
    closeMenu();
    switcherButton?.focus();
  }
</script>

<button
  bind:this={switcherButton}
  type="button"
  class={cn(
    "mobile-chrome-button flex min-w-0 items-center justify-start gap-1.5 px-1.5 text-foreground transition-colors hover:bg-accent",
    menuOpen && "bg-accent",
  )}
  aria-label={activeLabel}
  title={activeLabel}
  aria-haspopup="menu"
  aria-expanded={menuOpen}
  data-project-mobile-view-switcher
  onclick={toggleMenu}
>
  {#if ActiveIcon}<ActiveIcon class="shrink-0" size={16} strokeWidth={1.75} aria-hidden="true" />{/if}
  <span class="min-w-0 truncate text-xs font-medium">{activeLabel}</span>
  <ChevronDown class="shrink-0" size={11} aria-hidden="true" />
</button>

{#if menuOpen}
  <div
    use:portal
    use:anchoredPopover={{ anchor: switcherButton ?? undefined, onOutside: closeMenu }}
    class="surface-floating fixed z-90 flex w-floating-sm flex-col overflow-hidden"
    role="menu"
    tabindex="-1"
    aria-label={t("projects.toolbar.views")}
    data-app-floating-surface
    onkeydown={handleMenuKeydown}
  >
    <div class="surface-floating-body flex min-h-0 flex-1 flex-col overflow-y-auto overflow-x-hidden" use:scrollEdgeFadeAction>
      {#each views as view (view.id)}
        {@const Icon = PROJECT_VIEW_TYPE_ICONS[view.viewType]}
        {@const active = view.id === activeView?.id}
        <button
          type="button"
          role="menuitemradio"
          aria-checked={active}
          class="menu-item"
          onclick={() => selectView(view)}
        >
          <Icon class="text-muted-foreground" />
          <span class="min-w-0 flex-1 truncate">{projectViewLabel(t, view)}</span>
          {#if active}<Check class="size-3.5 shrink-0" aria-hidden="true" />{/if}
        </button>
      {:else}
        <p class="px-2 py-1 text-panel-detail text-muted-foreground">{t("projects.sidebar.noViews")}</p>
      {/each}
      {#if addableViewTypes.length > 0}
        <div role="separator" class="menu-separator"></div>
        <button
          type="button"
          role="menuitem"
          class="menu-item"
          aria-expanded={addOpen}
          data-project-mobile-add-view
          onclick={toggleAdd}
        >
          <Plus class="text-muted-foreground" />
          <span class="min-w-0 flex-1 truncate">{t("projects.sidebar.addView")}</span>
          {#if addOpen}<ChevronDown aria-hidden="true" />{:else}<ChevronRight aria-hidden="true" />{/if}
        </button>
        {#if addOpen}
          <div
            bind:this={addGroup}
            class="flex flex-col border-y border-border bg-muted/25 py-1"
            role="group"
            aria-label={t("projects.sidebar.addView")}
          >
            {#each addableViewTypes as viewType (viewType)}
              {@const Icon = PROJECT_VIEW_TYPE_ICONS[viewType]}
              <button
                type="button"
                role="menuitem"
                class="menu-item"
                data-project-add-view={viewType}
                onclick={() => void addView(viewType)}
              >
                <Icon class="text-muted-foreground" />
                <span class="min-w-0 flex-1 truncate">{projectViewTypeLabel(t, viewType)}</span>
              </button>
            {/each}
          </div>
        {/if}
      {/if}
    </div>
  </div>
{/if}
