<script lang="ts">
  import { onDestroy } from "svelte";
  import Archive from "@lucide/svelte/icons/archive";
  import History from "@lucide/svelte/icons/history";
  import TriangleAlert from "@lucide/svelte/icons/triangle-alert";
  import Trash2 from "@lucide/svelte/icons/trash-2";
  import {
    getNotesHistoryRetentionImpact,
    pruneNotesProjectHistory,
  } from "$lib/api/notes/project-history";
  import ConfirmDialog from "$lib/components/ui/ConfirmDialog.svelte";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import {
    isNotesPageOpenMode,
    type NotesPageOpenMode,
  } from "$lib/notes/pages/open-mode";
  import {
    DEFAULT_NOTES_HISTORY_RETENTION_DAYS,
    effectiveNotesHistoryRetentionDays,
    isNotesHistoryRetentionDays,
    type NotesHistoryRetentionDays,
  } from "$lib/notes/history/retention";
  import {
    PROJECT_SETTINGS_PANEL_MAX_HEIGHT,
    projectToolbarPanelGeometry,
  } from "$lib/projects/toolbar";
  import { getProjects } from "$lib/stores/projects.svelte";
  import { getNotes } from "$lib/stores/notes.svelte";
  import { getViewport } from "$lib/stores/viewport.svelte";
  import {
    APP_FLOATING_SURFACE_SELECTOR,
    isAppFloatingSurfaceTarget,
  } from "$lib/utils";
  import Select from "$lib/components/ui/Select.svelte";
  import ProjectSettingsPanelShell from "$lib/components/projects/settings/ProjectSettingsPanelShell.svelte";
  import ProjectSettingsSectionHeading from "$lib/components/projects/settings/ProjectSettingsSectionHeading.svelte";

  type NotesProjectDefaultOpenMode = "default" | NotesPageOpenMode;
  type NotesProjectHistoryRetention = "global" | NotesHistoryRetentionDays;

  let {
    projectId,
    popoverBoundaryElement,
    onRequestClose,
    onDirtyChange,
    onOpenVersionHistory,
    onOpenArchive,
    onOpenTrash,
  }: {
    projectId: string;
    popoverBoundaryElement: HTMLElement | null;
    onRequestClose: () => void;
    onDirtyChange: (dirty: boolean) => void;
    onOpenVersionHistory: () => void;
    onOpenArchive: () => void;
    onOpenTrash: () => void;
  } = $props();

  const projects = getProjects();
  const notes = getNotes();
  const viewport = getViewport();
  const { t } = getLocalization();

  let panelElement = $state<HTMLDivElement | null>(null);
  let scrollElement = $state<HTMLElement | undefined>();
  let panelStyle = $state("");
  let panelGeometryFrame: number | null = null;
  let draftProjectId = $state<string | null>(null);
  let draftProjectUpdatedAt = $state<string | null>(null);
  let defaultOpenModeDraft = $state<NotesProjectDefaultOpenMode>("default");
  let historyRetentionDraft = $state<NotesProjectHistoryRetention>("global");
  let saving = $state(false);
  let error = $state<string | null>(null);
  let pruneConfirmationOpen = $state(false);
  let pruneVersionCount = $state(0);
  let pruneStoredBytes = $state(0);

  const selectedProject = $derived(projects.projectById(projectId));
  const projectSettingsTitle = $derived(
    selectedProject ? t("notes.projectSettingsTitle", selectedProject.name) : "",
  );
  const draftReady = $derived(Boolean(selectedProject && draftProjectId === selectedProject.id));
  const persistedDefaultOpenMode = $derived<NotesProjectDefaultOpenMode>(
    selectedProject?.notesDefaultOpenMode ?? "default",
  );
  const persistedHistoryRetention = $derived<NotesProjectHistoryRetention>(
    selectedProject?.notesHistoryRetentionDays ?? "global",
  );
  const globalHistoryRetention = $derived(
    notes.pageHistorySettings?.retention_days ?? DEFAULT_NOTES_HISTORY_RETENTION_DAYS,
  );
  const effectiveHistoryRetentionDraft = $derived(
    effectiveNotesHistoryRetentionDays(
      globalHistoryRetention,
      historyRetentionDraft === "global" ? null : historyRetentionDraft,
    ),
  );
  const dirty = $derived(
    draftReady
      && (
        defaultOpenModeDraft !== persistedDefaultOpenMode
        || historyRetentionDraft !== persistedHistoryRetention
      ),
  );
  const openModeOptions = $derived([
    { value: "default", label: t("notes.projectSettingsUseGlobal") },
    { value: "center", label: t("notes.centerPeek") },
    { value: "side", label: t("notes.sidePeek") },
    { value: "full", label: t("notes.fullPage") },
  ]);
  const historyRetentionOptions = $derived([
    { value: "global", label: t("notes.projectSettingsUseGlobal") },
    { value: "0", label: t("notes.projectSettingsHistoryOff") },
    { value: "7", label: t("notes.projectSettingsHistory7") },
    { value: "30", label: t("notes.projectSettingsHistory30") },
    { value: "90", label: t("notes.projectSettingsHistory90") },
    { value: "180", label: t("notes.projectSettingsHistory180") },
    { value: "365", label: t("notes.projectSettingsHistory365") },
  ]);

  function loadDraft(): void {
    if (!selectedProject) return;
    draftProjectId = selectedProject.id;
    draftProjectUpdatedAt = selectedProject.updatedAt;
    defaultOpenModeDraft = selectedProject.notesDefaultOpenMode ?? "default";
    historyRetentionDraft = selectedProject.notesHistoryRetentionDays ?? "global";
    error = null;
  }

  function refreshPanelGeometry(): void {
    panelGeometryFrame = null;
    const trigger = document.querySelector<HTMLElement>("[data-notes-toolbar-trigger='settings']");
    if (!trigger) return;
    const rect = trigger.getBoundingClientRect();
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

  function requestPanelGeometryRefresh(): void {
    if (panelGeometryFrame !== null) cancelAnimationFrame(panelGeometryFrame);
    panelGeometryFrame = requestAnimationFrame(refreshPanelGeometry);
  }

  function discard(): void {
    loadDraft();
  }

  function close(): void {
    onRequestClose();
  }

  function selectDefaultOpenMode(value: string): void {
    if (value === "default" || isNotesPageOpenMode(value)) {
      defaultOpenModeDraft = value;
    }
  }

  function selectHistoryRetention(value: string): void {
    if (value === "global") {
      historyRetentionDraft = value;
      return;
    }
    const parsed = Number.parseInt(value, 10);
    if (isNotesHistoryRetentionDays(parsed)) historyRetentionDraft = parsed;
  }

  function formatStoredBytes(value: number): string {
    if (value < 1024) return `${value} B`;
    const units = ["KB", "MB", "GB", "TB"];
    let size = value / 1024;
    let unitIndex = 0;
    while (size >= 1024 && unitIndex < units.length - 1) {
      size /= 1024;
      unitIndex += 1;
    }
    return `${new Intl.NumberFormat(undefined, { maximumFractionDigits: 1 }).format(size)} ${units[unitIndex]}`;
  }

  async function save(): Promise<void> {
    if (!selectedProject || !dirty || saving) return;
    saving = true;
    error = null;
    try {
      const currentEffective = effectiveNotesHistoryRetentionDays(
        globalHistoryRetention,
        selectedProject.notesHistoryRetentionDays,
      );
      const nextEffective = effectiveNotesHistoryRetentionDays(
        globalHistoryRetention,
        historyRetentionDraft === "global" ? null : historyRetentionDraft,
      );
      if (nextEffective < currentEffective) {
        const impact = await getNotesHistoryRetentionImpact(nextEffective, selectedProject.id);
        if (impact.versionCount > 0) {
          pruneVersionCount = impact.versionCount;
          pruneStoredBytes = impact.storedBytes;
          pruneConfirmationOpen = true;
          return;
        }
      }
      await saveConfirmed(nextEffective < currentEffective);
    } catch (cause) {
      const message = cause instanceof Error ? cause.message : String(cause);
      error = t("notes.projectSettingsSaveFailed", message);
    } finally {
      saving = false;
    }
  }

  async function saveConfirmed(pruneAfterSave: boolean): Promise<void> {
    if (!selectedProject) return;
    const openMode = defaultOpenModeDraft === "default" ? null : defaultOpenModeDraft;
    const retention = historyRetentionDraft === "global" ? null : historyRetentionDraft;
    await projects.setNotesSettings(selectedProject.id, openMode, retention);
    if (pruneAfterSave) await pruneNotesProjectHistory(selectedProject.id);
  }

  function handleWindowPointerDown(event: PointerEvent): void {
    const target = event.target;
    if (!(target instanceof Node)) return;
    if (target instanceof Element && target.closest("[role='dialog'][aria-modal='true']")) return;
    const trigger = document.querySelector<HTMLElement>("[data-notes-toolbar-trigger='settings']");
    if (isAppFloatingSurfaceTarget(target) || trigger?.contains(target) || panelElement?.contains(target)) {
      return;
    }
    close();
  }

  function handleWindowKeydown(event: KeyboardEvent): void {
    if (event.key !== "Escape") return;
    if (isAppFloatingSurfaceTarget(event.target) || document.querySelector(APP_FLOATING_SURFACE_SELECTOR)) {
      return;
    }
    event.preventDefault();
    close();
  }

  $effect(() => {
    if (!selectedProject) return;
    if (
      draftProjectId !== selectedProject.id
      || (!dirty && draftProjectUpdatedAt !== selectedProject.updatedAt)
    ) {
      loadDraft();
    }
  });

  $effect(() => {
    if (notes.pageHistorySettings || notes.pageHistorySettingsLoading) return;
    void notes.ensureOptionalSubsystem("history-settings");
  });

  $effect(() => {
    onDirtyChange(dirty);
  });

  onDestroy(() => {
    onDirtyChange(false);
  });

  $effect(() => {
    const viewportWidth = viewport.width;
    const viewportHeight = viewport.height;
    void viewportWidth;
    void viewportHeight;
    requestPanelGeometryRefresh();
    return () => {
      if (panelGeometryFrame !== null) {
        cancelAnimationFrame(panelGeometryFrame);
        panelGeometryFrame = null;
      }
    };
  });
</script>

<svelte:window onpointerdown={handleWindowPointerDown} onkeydown={handleWindowKeydown} />

{#if selectedProject}
  <div
    bind:this={panelElement}
    class="surface-floating fixed z-80 flex min-h-0 flex-col overflow-hidden"
    style={panelStyle}
    role="dialog"
    tabindex="-1"
    aria-label={projectSettingsTitle}
    data-app-shortcuts="ignore"
  >
    <ProjectSettingsPanelShell
      presentation="popover"
      {draftReady}
      {dirty}
      {saving}
      {error}
      title={projectSettingsTitle}
      discardLabel={t("projects.settings.discard")}
      closeLabel={t("projects.settings.close")}
      saveLabel={t("projects.settings.save")}
      onDiscard={discard}
      onClose={close}
      onSave={() => { void save(); }}
      bind:scrollElement
    >
      <section class="flex flex-col gap-2">
        <div class="h-px bg-border/70" aria-hidden="true"></div>
        <div class="flex flex-col gap-1.5">
          <ProjectSettingsSectionHeading label={t("settings.section.appearance")} />
          <Select
            label={t("notes.projectSettingsDefaultViewMode")}
            value={defaultOpenModeDraft}
            options={openModeOptions}
            onChange={selectDefaultOpenMode}
            {popoverBoundaryElement}
            class="w-44"
          />
        </div>
      </section>

      <div class="h-px bg-border/70" aria-hidden="true"></div>

      <section class="flex flex-col gap-1.5">
        <ProjectSettingsSectionHeading label={t("notes.projectSettingsRestore")} />
        <Select
          label={t("notes.projectSettingsHistoryRetention")}
          value={String(historyRetentionDraft)}
          options={historyRetentionOptions}
          onChange={selectHistoryRetention}
          {popoverBoundaryElement}
          showSelectedSummary={false}
          class="w-44"
        />
        {#if effectiveHistoryRetentionDraft === 365}
          <div class="flex items-center gap-1.5 px-1 text-panel-detail text-warning">
            <TriangleAlert class="size-3.5 shrink-0" />
            <span>{t("settings.notesGeneral.historyRetention365Warning")}</span>
          </div>
        {/if}
        <div class="flex flex-wrap justify-start gap-2 px-1 py-1">
          <button
            type="button"
            class="flex min-h-(--panel-row-height) items-center gap-1.5 rounded-floating-item border border-border px-2.5 font-medium text-popover-foreground transition-colors hover:bg-accent"
            onclick={onOpenVersionHistory}
          >
            <History size={13} strokeWidth={1.75} />
            <span>{t("notes.projectSettingsVersionHistory")}</span>
          </button>
          <button
            type="button"
            class="flex min-h-(--panel-row-height) items-center gap-1.5 rounded-floating-item border border-border px-2.5 font-medium text-popover-foreground transition-colors hover:bg-accent"
            onclick={onOpenArchive}
          >
            <Archive size={13} strokeWidth={1.75} />
            <span>{t("notes.projectSettingsArchivedNotes")}</span>
          </button>
          <button
            type="button"
            class="flex min-h-(--panel-row-height) items-center gap-1.5 rounded-floating-item border border-border px-2.5 font-medium text-popover-foreground transition-colors hover:bg-accent"
            onclick={onOpenTrash}
          >
            <Trash2 size={13} strokeWidth={1.75} />
            <span>{t("notes.projectSettingsDeletedNotes")}</span>
          </button>
        </div>
      </section>
    </ProjectSettingsPanelShell>
  </div>
{/if}

{#if pruneConfirmationOpen}
  <ConfirmDialog
    title={t("notes.projectSettingsPruneTitle")}
    message={t(
      "notes.projectSettingsPruneMessage",
      pruneVersionCount,
      formatStoredBytes(pruneStoredBytes),
    )}
    confirmLabel={t("projects.settings.save")}
    cancelLabel={t("common.cancel")}
    onConfirm={() => {
      pruneConfirmationOpen = false;
      saving = true;
      void saveConfirmed(true).catch((cause) => {
        const message = cause instanceof Error ? cause.message : String(cause);
        error = t("notes.projectSettingsSaveFailed", message);
      }).finally(() => {
        saving = false;
      });
    }}
    onCancel={() => {
      pruneConfirmationOpen = false;
      saving = false;
    }}
  />
{/if}
