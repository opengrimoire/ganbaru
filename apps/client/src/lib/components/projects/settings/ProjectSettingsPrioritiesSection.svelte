<script lang="ts">
  import GripVertical from "@lucide/svelte/icons/grip-vertical";
  import Plus from "@lucide/svelte/icons/plus";
  import Trash2 from "@lucide/svelte/icons/trash-2";
  import EventColorPicker from "$lib/components/calendar/EventColorPicker.svelte";
  import type { EventColor } from "$lib/calendar/types";
  import { moveTextInputCaretToPointer } from "$lib/utils/text-input-caret";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import type { ProjectSettingsDropPosition } from "$lib/projects/settings/reorder";
  import { projectSettingsIconButtonClass } from "$lib/projects/settings/ui";
  import type { MoveDirection, ProjectPriorityConfig } from "$lib/projects/types";
  import type { Theme } from "$lib/themes";
  import { cn, type MaybePromise } from "$lib/utils";
  import ProjectSettingsNewRowDragHandle from "./ProjectSettingsNewRowDragHandle.svelte";
  import ProjectSettingsSectionHeading from "./ProjectSettingsSectionHeading.svelte";


  let {
    theme,
    priorities,
    draggedPriorityId,
    priorityReorderPending,
    newPriorityRowElement = $bindable<HTMLDivElement | undefined>(),
    newPriorityName = $bindable<string>(),
    newPriorityColor = $bindable<EventColor>(),
    priorityDropMarkerVisible,
    onPriorityDragOver,
    onPriorityDrop,
    onPriorityDragStart,
    clearPriorityDrag,
    movePriorityByDirection,
    priorityColorDraftValue,
    setPriorityColor,
    priorityNameDraftValue,
    setPriorityNameDraft,
    priorityDeleteDisabled,
    priorityDeleteTitle,
    requestDeletePriority,
    setNewPriorityColor,
    submitPriority,
  }: {
    theme: Theme;
    priorities: ProjectPriorityConfig[];
    draggedPriorityId: string | null;
    priorityReorderPending: boolean;
    newPriorityRowElement: HTMLDivElement | undefined;
    newPriorityName: string;
    newPriorityColor: EventColor;
    priorityDropMarkerVisible: (priorityId: string, position: ProjectSettingsDropPosition) => boolean;
    onPriorityDragOver: (event: DragEvent, priority: ProjectPriorityConfig, target: HTMLElement) => void;
    onPriorityDrop: (event: DragEvent, priority: ProjectPriorityConfig) => MaybePromise;
    onPriorityDragStart: (event: DragEvent, priority: ProjectPriorityConfig) => void;
    clearPriorityDrag: () => void;
    movePriorityByDirection: (priority: ProjectPriorityConfig, direction: MoveDirection) => MaybePromise;
    priorityColorDraftValue: (priority: ProjectPriorityConfig) => EventColor;
    setPriorityColor: (priorityId: string, color: EventColor | undefined) => void;
    priorityNameDraftValue: (priority: ProjectPriorityConfig) => string;
    setPriorityNameDraft: (priorityId: string, name: string) => void;
    priorityDeleteDisabled: (priority: ProjectPriorityConfig) => boolean;
    priorityDeleteTitle: (priority: ProjectPriorityConfig) => string;
    requestDeletePriority: (priority: ProjectPriorityConfig) => void;
    setNewPriorityColor: (color: EventColor | undefined) => void;
    submitPriority: () => MaybePromise;
  } = $props();

  const { t } = getLocalization();
</script>

<section class="flex flex-col gap-0.5">
  <ProjectSettingsSectionHeading label={t("projects.settings.taskPriorities")} />
  <div class="flex flex-col gap-2">
    {#each priorities as priority (priority.id)}
      {@const deletePriorityTitle = priorityDeleteTitle(priority)}
      <div
        class={cn(
          "relative grid min-h-7 grid-cols-[auto_auto_minmax(0,1fr)_auto] items-center gap-1.5 px-1 py-0.5",
          draggedPriorityId === priority.id && "opacity-50",
        )}
        role="group"
        aria-label={priority.name}
        ondragover={(event) => onPriorityDragOver(event, priority, event.currentTarget)}
        ondrop={(event) => { void onPriorityDrop(event, priority); }}
      >
        {#if priorityDropMarkerVisible(priority.id, "before")}
          <div class="pointer-events-none absolute left-1 right-1 top-0 h-0.5 rounded-full bg-primary"></div>
        {/if}
        {#if priorityDropMarkerVisible(priority.id, "after")}
          <div class="pointer-events-none absolute bottom-0 left-1 right-1 h-0.5 rounded-full bg-primary"></div>
        {/if}
        <button
          type="button"
          class="flex h-7 w-7 shrink-0 cursor-grab items-center justify-center rounded-md text-muted-foreground hover:bg-accent hover:text-foreground active:cursor-grabbing disabled:cursor-not-allowed disabled:opacity-40"
          draggable={priorities.length > 1 && !priorityReorderPending}
          disabled={priorities.length <= 1 || priorityReorderPending}
          aria-label={t("projects.actions.dragPriority", priority.name)}
          ondragstart={(event) => onPriorityDragStart(event, priority)}
          ondragend={clearPriorityDrag}
          onkeydown={(event) => {
            if (event.key === "ArrowUp") {
              event.preventDefault();
              void movePriorityByDirection(priority, -1);
            }
            if (event.key === "ArrowDown") {
              event.preventDefault();
              void movePriorityByDirection(priority, 1);
            }
          }}
        >
          <GripVertical size={13} strokeWidth={1.75} />
        </button>
        <EventColorPicker
          color={priorityColorDraftValue(priority)}
          {theme}
          ariaLabel={t("projects.settings.selectPriorityColor", priority.name)}
          class="h-7 w-7 justify-center self-center"
          buttonClass="size-6 rounded-md"
          onSelect={(color) => setPriorityColor(priority.id, color)}
        />
        <input
          value={priorityNameDraftValue(priority)}
          class="field min-w-0 text-foreground"
          aria-label={t("projects.settings.priorityName")}
          onpointerdown={moveTextInputCaretToPointer}
          oninput={(event) => setPriorityNameDraft(priority.id, event.currentTarget.value)}
        />
        <button
          type="button"
          class={projectSettingsIconButtonClass("danger")}
          disabled={priorityDeleteDisabled(priority)}
          aria-label={deletePriorityTitle}
          title={deletePriorityTitle}
          onclick={() => requestDeletePriority(priority)}
        >
          <Trash2 size={13} strokeWidth={1.75} />
        </button>
      </div>
    {/each}

    <div
      bind:this={newPriorityRowElement}
      class="grid min-h-7 grid-cols-[auto_auto_minmax(0,1fr)_auto] items-center gap-1.5 px-1 py-0.5"
    >
      <ProjectSettingsNewRowDragHandle showIcon={priorities.length === 0} />
      <EventColorPicker
        color={newPriorityColor}
        {theme}
        ariaLabel={t("projects.settings.selectNewPriorityColor")}
        class="h-7 w-7 justify-center self-center"
        buttonClass="size-6 rounded-md"
        onSelect={setNewPriorityColor}
      />
      <input
        bind:value={newPriorityName}
        class="field min-w-0 text-foreground"
        placeholder={t("projects.settings.newPriorityPlaceholder")}
        onpointerdown={moveTextInputCaretToPointer}
        onkeydown={(event) => {
          if (event.key === "Enter") {
            event.preventDefault();
            void submitPriority();
          }
        }}
      />
      <button
        type="button"
        class="flex h-7 w-7 shrink-0 items-center justify-center rounded-md bg-primary text-primary-foreground disabled:cursor-not-allowed disabled:opacity-50"
        aria-label={t("projects.settings.addPriority")}
        title={t("projects.settings.addPriority")}
        onclick={() => { void submitPriority(); }}
      >
        <Plus size={13} strokeWidth={1.75} />
      </button>
    </div>
  </div>
</section>
