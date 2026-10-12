<script lang="ts">
  import GripVertical from "@lucide/svelte/icons/grip-vertical";
  import Plus from "@lucide/svelte/icons/plus";
  import Trash2 from "@lucide/svelte/icons/trash-2";
  import EventColorPicker from "$lib/components/calendar/EventColorPicker.svelte";
  import type { EventColor } from "$lib/calendar/types";
  import Select from "$lib/components/ui/Select.svelte";
  import { moveTextInputCaretToPointer } from "$lib/utils/text-input-caret";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import type { ProjectSettingsDropPosition } from "$lib/projects/settings/reorder";
  import { projectSettingsIconButtonClass } from "$lib/projects/settings/ui";
  import type { MoveDirection, ProjectStatus, ProjectStatusCategory } from "$lib/projects/types";
  import type { Theme } from "$lib/themes";
  import { cn, type MaybePromise } from "$lib/utils";
  import ProjectSettingsNewRowDragHandle from "./ProjectSettingsNewRowDragHandle.svelte";
  import ProjectSettingsSectionHeading from "./ProjectSettingsSectionHeading.svelte";

  type SelectOption = { value: string; label: string };

  let {
    theme,
    statuses,
    statusCategoryOptions,
    draggedStatusId,
    statusReorderPending,
    newStatusRowElement = $bindable<HTMLDivElement | undefined>(),
    newStatusName = $bindable<string>(),
    newStatusColor = $bindable<EventColor>(),
    newStatusCategory = $bindable<ProjectStatusCategory>(),
    statusDropMarkerVisible,
    onStatusDragOver,
    onStatusDrop,
    onStatusDragStart,
    clearStatusDrag,
    moveStatusByDirection,
    statusColorDraftValue,
    setStatusColor,
    statusNameDraftValue,
    setStatusNameDraft,
    statusCategoryDraftValue,
    setStatusCategory,
    statusDeleteDisabled,
    statusDeleteTitle,
    requestDeleteStatus,
    setNewStatusColor,
    setNewStatusCategory,
    submitStatus,
  }: {
    theme: Theme;
    statuses: ProjectStatus[];
    statusCategoryOptions: SelectOption[];
    draggedStatusId: string | null;
    statusReorderPending: boolean;
    newStatusRowElement: HTMLDivElement | undefined;
    newStatusName: string;
    newStatusColor: EventColor;
    newStatusCategory: ProjectStatusCategory;
    statusDropMarkerVisible: (statusId: string, position: ProjectSettingsDropPosition) => boolean;
    onStatusDragOver: (event: DragEvent, status: ProjectStatus, target: HTMLElement) => void;
    onStatusDrop: (event: DragEvent, status: ProjectStatus) => MaybePromise;
    onStatusDragStart: (event: DragEvent, status: ProjectStatus) => void;
    clearStatusDrag: () => void;
    moveStatusByDirection: (status: ProjectStatus, direction: MoveDirection) => MaybePromise;
    statusColorDraftValue: (status: ProjectStatus) => EventColor;
    setStatusColor: (statusId: string, color: EventColor | undefined) => void;
    statusNameDraftValue: (status: ProjectStatus) => string;
    setStatusNameDraft: (statusId: string, name: string) => void;
    statusCategoryDraftValue: (status: ProjectStatus) => ProjectStatusCategory;
    setStatusCategory: (statusId: string, value: string) => void;
    statusDeleteDisabled: (status: ProjectStatus) => boolean;
    statusDeleteTitle: (status: ProjectStatus) => string;
    requestDeleteStatus: (status: ProjectStatus) => void;
    setNewStatusColor: (color: EventColor | undefined) => void;
    setNewStatusCategory: (value: string) => void;
    submitStatus: () => MaybePromise;
  } = $props();

  const { t } = getLocalization();
</script>

<section class="flex flex-col gap-0.5">
  <ProjectSettingsSectionHeading label={t("projects.settings.taskStatuses")} />
  <div class="flex flex-col gap-0.5">
    {#each statuses as status (status.id)}
      {@const deleteStatusTitle = statusDeleteTitle(status)}
      <div
        class={cn(
          "relative grid min-h-7 grid-cols-[auto_auto_minmax(0,1fr)_auto_auto] items-center gap-1 px-1 py-0.5",
          draggedStatusId === status.id && "opacity-50",
        )}
        role="group"
        aria-label={status.name}
        ondragover={(event) => onStatusDragOver(event, status, event.currentTarget)}
        ondrop={(event) => { void onStatusDrop(event, status); }}
      >
        {#if statusDropMarkerVisible(status.id, "before")}
          <div class="pointer-events-none absolute left-1 right-1 top-0 h-0.5 rounded-full bg-primary"></div>
        {/if}
        {#if statusDropMarkerVisible(status.id, "after")}
          <div class="pointer-events-none absolute bottom-0 left-1 right-1 h-0.5 rounded-full bg-primary"></div>
        {/if}
        <button
          type="button"
          class="flex h-7 w-7 shrink-0 cursor-grab items-center justify-center rounded-md text-muted-foreground hover:bg-accent hover:text-foreground active:cursor-grabbing disabled:cursor-not-allowed disabled:opacity-40"
          draggable={statuses.length > 1 && !statusReorderPending}
          disabled={statuses.length <= 1 || statusReorderPending}
          aria-label={t("projects.actions.dragStatus", status.name)}
          ondragstart={(event) => onStatusDragStart(event, status)}
          ondragend={clearStatusDrag}
          onkeydown={(event) => {
            if (event.key === "ArrowUp") {
              event.preventDefault();
              void moveStatusByDirection(status, -1);
            }
            if (event.key === "ArrowDown") {
              event.preventDefault();
              void moveStatusByDirection(status, 1);
            }
          }}
        >
          <GripVertical size={13} strokeWidth={1.75} />
        </button>
        <EventColorPicker
          color={statusColorDraftValue(status)}
          {theme}
          ariaLabel={t("projects.settings.selectStatusColor", status.name)}
          class="h-7 w-7 justify-center self-center"
          buttonClass="size-6 rounded-md"
          onSelect={(color) => setStatusColor(status.id, color)}
        />
        <input
          value={statusNameDraftValue(status)}
          class="field min-w-0 text-foreground"
          aria-label={t("projects.settings.statusName")}
          onpointerdown={moveTextInputCaretToPointer}
          oninput={(event) => setStatusNameDraft(status.id, event.currentTarget.value)}
        />
        <Select
          value={statusCategoryDraftValue(status)}
          options={statusCategoryOptions}
          onChange={(value) => setStatusCategory(status.id, value)}
          ariaLabel={t("projects.settings.statusCategory")}
          class="w-32"
        />
        <button
          type="button"
          class={projectSettingsIconButtonClass("danger")}
          disabled={statusDeleteDisabled(status)}
          aria-label={deleteStatusTitle}
          title={deleteStatusTitle}
          onclick={() => requestDeleteStatus(status)}
        >
          <Trash2 size={13} strokeWidth={1.75} />
        </button>
      </div>
    {/each}
  </div>

  <div
    bind:this={newStatusRowElement}
    class="grid min-h-7 grid-cols-[auto_auto_minmax(0,1fr)_auto_auto] items-center gap-1 px-1 py-0.5"
  >
    <ProjectSettingsNewRowDragHandle showIcon={statuses.length === 0} />
    <EventColorPicker
      color={newStatusColor}
      {theme}
      ariaLabel={t("projects.settings.selectNewStatusColor")}
      class="h-7 w-7 justify-center self-center"
      buttonClass="size-6 rounded-md"
      onSelect={setNewStatusColor}
    />
    <input
      bind:value={newStatusName}
      class="field min-w-0 text-foreground"
      placeholder={t("projects.settings.newStatusPlaceholder")}
      onpointerdown={moveTextInputCaretToPointer}
      onkeydown={(event) => {
        if (event.key === "Enter") {
          event.preventDefault();
          void submitStatus();
        }
      }}
    />
    <Select
      value={newStatusCategory}
      options={statusCategoryOptions}
      onChange={setNewStatusCategory}
      ariaLabel={t("projects.settings.statusCategory")}
      class="w-32"
    />
    <button
      type="button"
      class="flex h-7 w-7 shrink-0 items-center justify-center rounded-md bg-primary text-primary-foreground disabled:cursor-not-allowed disabled:opacity-50"
      aria-label={t("projects.settings.addStatus")}
      title={t("projects.settings.addStatus")}
      onclick={() => { void submitStatus(); }}
    >
      <Plus size={13} strokeWidth={1.75} />
    </button>
  </div>
</section>
