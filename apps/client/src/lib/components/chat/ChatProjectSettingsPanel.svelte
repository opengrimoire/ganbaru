<script lang="ts">
  import Archive from "@lucide/svelte/icons/archive";
  import ProjectSettingsPanelShell from "$lib/components/projects/settings/ProjectSettingsPanelShell.svelte";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import {
    PROJECT_SETTINGS_PANEL_MAX_HEIGHT,
    projectToolbarPanelGeometry,
  } from "$lib/projects/toolbar";
  import { getViewport } from "$lib/stores/viewport.svelte";
  import {
    APP_FLOATING_SURFACE_SELECTOR,
    isAppFloatingSurfaceTarget,
  } from "$lib/utils";
  import { portal } from "$lib/utils/portal";

  let {
    projectName,
    triggerElement,
    onClose,
    onOpenArchive,
  }: {
    projectName: string;
    triggerElement: HTMLButtonElement;
    onClose: () => void;
    onOpenArchive: () => void;
  } = $props();

  const { t } = getLocalization();
  const viewport = getViewport();
  const title = $derived(t("chat.projectSettingsTitle", projectName));
  let panelElement = $state<HTMLDivElement | null>(null);
  let panelStyle = $state("visibility: hidden");
  let geometryFrame: number | null = null;

  function refreshGeometry(): void {
    geometryFrame = null;
    const rect = triggerElement.getBoundingClientRect();
    const geometry = projectToolbarPanelGeometry({
      anchorLeft: rect.left,
      anchorRight: rect.right,
      anchorTop: rect.top,
      anchorBottom: rect.bottom,
      viewportWidth: viewport.width,
      viewportHeight: viewport.height,
      preferredWidth: 430,
      preferredHeight: PROJECT_SETTINGS_PANEL_MAX_HEIGHT,
    });
    panelStyle = [
      `left: ${Math.round(geometry.left)}px`,
      `top: ${Math.round(geometry.top)}px`,
      `width: ${Math.round(geometry.width)}px`,
      `max-height: ${Math.round(geometry.maxHeight)}px`,
    ].join("; ");
  }

  function handleWindowPointerDown(event: PointerEvent): void {
    const target = event.target;
    if (!(target instanceof Node)) return;
    if (target instanceof Element && target.closest("[role='dialog'][aria-modal='true']")) return;
    if (isAppFloatingSurfaceTarget(target) || triggerElement.contains(target) || panelElement?.contains(target)) return;
    onClose();
  }

  function handleWindowKeydown(event: KeyboardEvent): void {
    if (event.key !== "Escape") return;
    if (isAppFloatingSurfaceTarget(event.target) || document.querySelector(APP_FLOATING_SURFACE_SELECTOR)) return;
    event.preventDefault();
    onClose();
  }

  $effect(() => {
    void viewport.width;
    void viewport.height;
    if (geometryFrame !== null) cancelAnimationFrame(geometryFrame);
    geometryFrame = requestAnimationFrame(refreshGeometry);
    return () => {
      if (geometryFrame !== null) {
        cancelAnimationFrame(geometryFrame);
        geometryFrame = null;
      }
    };
  });
</script>

<svelte:window onpointerdown={handleWindowPointerDown} onkeydown={handleWindowKeydown} />

<div
  use:portal
  bind:this={panelElement}
  data-chat-project-settings-panel
  class="surface-floating fixed z-80 flex min-h-0 flex-col overflow-hidden"
  style={panelStyle}
  role="dialog"
  tabindex="-1"
  aria-label={title}
  data-app-shortcuts="ignore"
>
  <ProjectSettingsPanelShell
    presentation="popover"
    draftReady={true}
    dirty={false}
    saving={false}
    error={null}
    {title}
    discardLabel={t("projects.settings.discard")}
    closeLabel={t("projects.settings.close")}
    saveLabel={t("projects.settings.save")}
    onDiscard={() => {}}
    {onClose}
    onSave={() => {}}
  >
    <section class="flex flex-col gap-2">
      <div class="h-px bg-border/70" aria-hidden="true"></div>
      <div class="flex flex-wrap justify-start gap-2 px-1 py-1">
        <button
          type="button"
          class="flex h-7 items-center gap-1.5 rounded-floating-item border border-border bg-transparent px-2.5 font-medium text-foreground transition-colors hover:bg-accent"
          onclick={onOpenArchive}
        >
          <Archive size={13} strokeWidth={1.75} />
          <span>{t("chat.channels.archive")}</span>
        </button>
      </div>
    </section>
  </ProjectSettingsPanelShell>
</div>
