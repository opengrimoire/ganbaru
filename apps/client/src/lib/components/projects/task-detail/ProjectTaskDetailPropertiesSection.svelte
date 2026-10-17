<script lang="ts">
  import Select from "$lib/components/ui/Select.svelte";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import { projectStatusBadgeDotStyle, projectTaskTypeLabel } from "$lib/projects/display";
  import { PROJECT_TASK_TYPES } from "$lib/projects/types";
  import type {
    ProjectPriority,
    ProjectPriorityConfig,
    ProjectSection,
    ProjectStatus,
    ProjectTaskType,
  } from "$lib/projects/types";
  import type { Theme } from "$lib/themes";
  import LocalPersonAvatar from "$lib/components/contacts/LocalPersonAvatar.svelte";
  import PriorityFlagIcon from "$lib/components/projects/PriorityFlagIcon.svelte";

  let {
    statuses,
    sections,
    priorities,
    theme,
    statusId,
    sectionId,
    priority,
    taskType,
    onStatusChange,
    onSectionChange,
    onPriorityChange,
    onTaskTypeChange,
  }: {
    statuses: ProjectStatus[];
    sections: ProjectSection[];
    priorities: ProjectPriorityConfig[];
    theme: Theme;
    statusId: string;
    sectionId: string;
    priority: ProjectPriority;
    taskType: ProjectTaskType;
    onStatusChange: (statusId: string) => void;
    onSectionChange: (sectionId: string) => void;
    onPriorityChange: (priority: ProjectPriority) => void;
    onTaskTypeChange: (taskType: ProjectTaskType) => void;
  } = $props();

  const { t } = getLocalization();
  const UNASSIGNED_VALUE = "unassigned";
  const LOCAL_PERSON_VALUE = "you";
  const PERSON_AVATAR_SIZE = 16;
  const personOptions = $derived([
    { value: UNASSIGNED_VALUE, label: t("projects.detail.unassigned") },
    { value: LOCAL_PERSON_VALUE, label: t("contacts.you") },
  ]);
</script>

{#snippet personLeading(value: string)}
  {#if value === LOCAL_PERSON_VALUE}<LocalPersonAvatar size={PERSON_AVATAR_SIZE} />{/if}
{/snippet}

<div class="grid gap-1">
  <div class="task-property-row">
    <span>{t("projects.detail.status")}</span>
    <Select inline appearance="quiet" contentAlign="start" class="w-full" value={statusId}
      ariaLabel={t("projects.detail.status")}
      options={statuses.map((status) => ({ value: status.id, label: status.name }))}
      onChange={onStatusChange}>
      {#snippet leading(value)}
        {@const status = statuses.find((entry) => entry.id === value)}
        <span class="size-2 shrink-0 rounded-full" style={projectStatusBadgeDotStyle(status, theme)}></span>
      {/snippet}
    </Select>
  </div>
  <div class="task-property-row">
    <span>{t("projects.detail.priority")}</span>
    <Select inline appearance="quiet" contentAlign="start" class="w-full" value={priority}
      ariaLabel={t("projects.detail.priority")}
      options={priorities.map((entry) => ({ value: entry.id, label: entry.name }))}
      onChange={(value) => {
        const option = priorities.find((entry) => entry.id === value);
        if (option) onPriorityChange(option.id);
      }}>
      {#snippet leading(value)}
        {@const option = priorities.find((entry) => entry.id === value)}
        {#if option}<PriorityFlagIcon color={option.color} {theme} size={14} />{/if}
      {/snippet}
    </Select>
  </div>
  <div class="task-property-row">
    <span>{t("projects.detail.section")}</span>
    <Select inline appearance="quiet" contentAlign="start" class="w-full" value={sectionId}
      ariaLabel={t("projects.detail.section")}
      options={sections.map((section) => ({ value: section.id, label: section.name }))}
      onChange={onSectionChange} />
  </div>
  <div class="task-property-row">
    <span>{t("projects.detail.type")}</span>
    <Select inline appearance="quiet" contentAlign="start" class="w-full" value={taskType}
      ariaLabel={t("projects.detail.type")}
      options={PROJECT_TASK_TYPES.map((value) => ({ value, label: projectTaskTypeLabel(value, t) }))}
      onChange={(value) => {
        const option = PROJECT_TASK_TYPES.find((entry) => entry === value);
        if (option) onTaskTypeChange(option);
      }} />
  </div>
  <div class="task-property-row">
    <span>{t("projects.detail.assignee")}</span>
    <Select inline appearance="quiet" contentAlign="start" class="w-full" value={UNASSIGNED_VALUE}
      ariaLabel={t("projects.detail.assignee")}
      options={personOptions}
      unavailable
      leading={personLeading}
      onChange={() => {}} />
  </div>
  <div class="task-property-row">
    <span>{t("projects.detail.reviewer")}</span>
    <Select inline appearance="quiet" contentAlign="start" class="w-full" value={UNASSIGNED_VALUE}
      ariaLabel={t("projects.detail.reviewer")}
      options={personOptions}
      unavailable
      leading={personLeading}
      onChange={() => {}} />
  </div>
</div>
