<script lang="ts">
  import Check from "@lucide/svelte/icons/check";
  import ChevronDown from "@lucide/svelte/icons/chevron-down";
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
   * Phone toolbar controls for the selected project's views: a switcher that names the open view and lists the others,
   * and a plus button that adds a view of a chosen type and opens it.
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

  type OpenMenu = "views" | "add" | null;

  let openMenu = $state<OpenMenu>(null);
  let switcherButton = $state<HTMLButtonElement | null>(null);
  let addButton = $state<HTMLButtonElement | null>(null);

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
    if (!openMenu) return;
    return mobileBackStack.activate({ handle: closeMenu });
  });

  function closeMenu(): void {
    openMenu = null;
  }

  function toggleMenu(menu: Exclude<OpenMenu, null>): void {
    openMenu = openMenu === menu ? null : menu;
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
    const trigger = openMenu === "add" ? addButton : switcherButton;
    closeMenu();
    trigger?.focus();
  }
</script>

<button
  bind:this={switcherButton}
  type="button"
  class={cn(
    "mobile-chrome-button flex min-w-0 items-center justify-start gap-1.5 px-1.5 text-foreground transition-colors hover:bg-accent",
    openMenu === "views" && "bg-accent",
  )}
  aria-label={activeLabel}
  title={activeLabel}
  aria-haspopup="menu"
  aria-expanded={openMenu === "views"}
  data-project-mobile-view-switcher
  onclick={() => toggleMenu("views")}
>
  {#if ActiveIcon}<ActiveIcon class="shrink-0" size={16} strokeWidth={1.75} aria-hidden="true" />{/if}
  <span class="min-w-0 truncate text-xs font-medium">{activeLabel}</span>
  <ChevronDown class="shrink-0" size={11} aria-hidden="true" />
</button>
<button
  bind:this={addButton}
  type="button"
  class={cn(
    "mobile-chrome-button flex w-(--mobile-chrome-column) shrink-0 items-center justify-center text-foreground transition-colors hover:bg-accent",
    openMenu === "add" && "bg-accent",
  )}
  aria-label={t("projects.sidebar.addView")}
  title={t("projects.sidebar.addView")}
  aria-haspopup="menu"
  aria-expanded={openMenu === "add"}
  data-project-mobile-add-view
  onclick={() => toggleMenu("add")}
>
  <Plus size={16} strokeWidth={1.75} />
</button>

{#if openMenu === "views"}
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
    </div>
  </div>
{:else if openMenu === "add"}
  <div
    use:portal
    use:anchoredPopover={{ anchor: addButton ?? undefined, onOutside: closeMenu }}
    class="surface-floating fixed z-90 flex w-floating-sm flex-col overflow-hidden"
    role="menu"
    tabindex="-1"
    aria-label={t("projects.sidebar.addView")}
    data-app-floating-surface
    onkeydown={handleMenuKeydown}
  >
    <div class="surface-floating-body flex min-h-0 flex-1 flex-col overflow-y-auto overflow-x-hidden" use:scrollEdgeFadeAction>
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
  </div>
{/if}
