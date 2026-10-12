<script lang="ts">
  import ChevronDown from "@lucide/svelte/icons/chevron-down";
  import GripVertical from "@lucide/svelte/icons/grip-vertical";
  import Plus from "@lucide/svelte/icons/plus";
  import Trash2 from "@lucide/svelte/icons/trash-2";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import {
    customFieldOptionConnectorPath,
    customFieldOptionConnectorStyle,
    customFieldOptionConnectorViewBox,
  } from "$lib/projects/custom-field-option-connector";
  import { projectCustomFieldTypeLabel } from "$lib/projects/display";
  import { projectSettingsIconButtonClass } from "$lib/projects/settings/ui";
  import { projectCustomFieldUsesOptions } from "$lib/projects/custom-fields";
  import type { ProjectSettingsDropPosition } from "$lib/projects/settings/reorder";
  import type {
    NewCustomFieldDraft,
    NewCustomFieldOptionDraft,
  } from "$lib/projects/settings/custom-field-drafts";
  import type {
    MoveDirection,
    ProjectCustomField,
    ProjectCustomFieldOption,
    ProjectCustomFieldType,
  } from "$lib/projects/types";
  import { moveTextInputCaretToPointer } from "$lib/utils/text-input-caret";
  import ProjectSettingsCustomFieldTypePicker from "./ProjectSettingsCustomFieldTypePicker.svelte";
  import ProjectSettingsNewRowDragHandle from "./ProjectSettingsNewRowDragHandle.svelte";
  import ProjectSettingsSectionHeading from "./ProjectSettingsSectionHeading.svelte";
  import { cn, type MaybePromise } from "$lib/utils";

  type CustomFieldDropPosition = ProjectSettingsDropPosition;
  type CustomFieldOptionDropPosition = ProjectSettingsDropPosition;

  let {
    projectCustomFields,
    customFieldOptions,
    customFieldOptionCreateDraftRows,
    customFieldNameDrafts = $bindable<Record<string, string>>(),
    customFieldOptionNameDrafts = $bindable<Record<string, string>>(),
    customFieldCreateDraftRows = $bindable<NewCustomFieldDraft[]>(),
    newCustomFieldName = $bindable<string>(),
    newCustomFieldType = $bindable<ProjectCustomFieldType>(),
    newCustomFieldOptionDrafts = $bindable<Record<string, string>>(),
    newCustomFieldOptionRows = $bindable<NewCustomFieldOptionDraft[]>(),
    newCustomFieldOptionName = $bindable<string>(),
    newCustomFieldRowElement = $bindable<HTMLDivElement | undefined>(),
    draggedCustomFieldId,
    draggedCustomFieldOptionId,
    customFieldReorderPending,
    customFieldOptionReorderPending,
    customFieldDropMarkerVisible,
    customFieldOptionDropMarkerVisible,
    onCustomFieldDragOver,
    onCustomFieldDrop,
    onCustomFieldDragStart,
    clearCustomFieldDrag,
    moveCustomFieldByDirection,
    requestDeleteCustomField,
    onCustomFieldOptionDragOver,
    onCustomFieldOptionDrop,
    onCustomFieldOptionDragStart,
    clearCustomFieldOptionDrag,
    moveCustomFieldOptionByDirection,
    requestDeleteCustomFieldOption,
    setCustomFieldOptionCreateDraftName,
    removeCustomFieldOptionCreateDraft,
    setCustomFieldCreateDraftName,
    setCustomFieldCreateDraftOptionName,
    setCustomFieldCreateDraftPendingOptionName,
    removeCustomFieldCreateDraft,
    removeCustomFieldCreateDraftOption,
    submitCustomFieldOption,
    submitCustomFieldCreateDraftOption,
    submitCustomField,
    setNewCustomFieldOptionDraftName,
    removeNewCustomFieldOptionDraft,
    submitNewCustomFieldOptionDraft,
  }: {
    projectCustomFields: ProjectCustomField[];
    customFieldOptions: (field: ProjectCustomField) => ProjectCustomFieldOption[];
    customFieldOptionCreateDraftRows: (fieldId: string) => NewCustomFieldOptionDraft[];
    customFieldNameDrafts: Record<string, string>;
    customFieldOptionNameDrafts: Record<string, string>;
    customFieldCreateDraftRows: NewCustomFieldDraft[];
    newCustomFieldName: string;
    newCustomFieldType: ProjectCustomFieldType;
    newCustomFieldOptionDrafts: Record<string, string>;
    newCustomFieldOptionRows: NewCustomFieldOptionDraft[];
    newCustomFieldOptionName: string;
    newCustomFieldRowElement: HTMLDivElement | undefined;
    draggedCustomFieldId: string | null;
    draggedCustomFieldOptionId: string | null;
    customFieldReorderPending: boolean;
    customFieldOptionReorderPending: boolean;
    customFieldDropMarkerVisible: (fieldId: string, position: CustomFieldDropPosition) => boolean;
    customFieldOptionDropMarkerVisible: (optionId: string, position: CustomFieldOptionDropPosition) => boolean;
    onCustomFieldDragOver: (
      event: DragEvent,
      field: ProjectCustomField,
      target: HTMLElement,
    ) => void;
    onCustomFieldDrop: (event: DragEvent, field: ProjectCustomField) => MaybePromise;
    onCustomFieldDragStart: (event: DragEvent, field: ProjectCustomField) => void;
    clearCustomFieldDrag: () => void;
    moveCustomFieldByDirection: (field: ProjectCustomField, direction: MoveDirection) => MaybePromise;
    requestDeleteCustomField: (field: ProjectCustomField) => void;
    onCustomFieldOptionDragOver: (
      event: DragEvent,
      option: ProjectCustomFieldOption,
      target: HTMLElement,
    ) => void;
    onCustomFieldOptionDrop: (event: DragEvent, option: ProjectCustomFieldOption) => MaybePromise;
    onCustomFieldOptionDragStart: (event: DragEvent, option: ProjectCustomFieldOption) => void;
    clearCustomFieldOptionDrag: () => void;
    moveCustomFieldOptionByDirection: (
      option: ProjectCustomFieldOption,
      direction: MoveDirection,
    ) => MaybePromise;
    requestDeleteCustomFieldOption: (option: ProjectCustomFieldOption) => void;
    setCustomFieldOptionCreateDraftName: (
      fieldId: string,
      optionId: string,
      name: string,
    ) => void;
    removeCustomFieldOptionCreateDraft: (fieldId: string, optionId: string) => void;
    setCustomFieldCreateDraftName: (fieldId: string, name: string) => void;
    setCustomFieldCreateDraftOptionName: (
      fieldId: string,
      optionId: string,
      name: string,
    ) => void;
    setCustomFieldCreateDraftPendingOptionName: (fieldId: string, optionName: string) => void;
    removeCustomFieldCreateDraft: (fieldId: string) => void;
    removeCustomFieldCreateDraftOption: (fieldId: string, optionId: string) => void;
    submitCustomFieldOption: (field: ProjectCustomField) => MaybePromise;
    submitCustomFieldCreateDraftOption: (field: NewCustomFieldDraft) => void;
    submitCustomField: () => void;
    setNewCustomFieldOptionDraftName: (optionId: string, name: string) => void;
    removeNewCustomFieldOptionDraft: (optionId: string) => void;
    submitNewCustomFieldOptionDraft: () => void;
  } = $props();

  const { t } = getLocalization();

  function customFieldNameDraftValue(field: ProjectCustomField): string {
    return customFieldNameDrafts[field.id] ?? field.name;
  }

  function customFieldOptionNameDraftValue(option: ProjectCustomFieldOption): string {
    return customFieldOptionNameDrafts[option.id] ?? option.name;
  }

  function setCustomFieldNameDraft(fieldId: string, name: string): void {
    customFieldNameDrafts = {
      ...customFieldNameDrafts,
      [fieldId]: name,
    };
  }

  function setCustomFieldOptionNameDraft(optionId: string, name: string): void {
    customFieldOptionNameDrafts = {
      ...customFieldOptionNameDrafts,
      [optionId]: name,
    };
  }

  function setNewOptionDraft(fieldId: string, name: string): void {
    newCustomFieldOptionDrafts = {
      ...newCustomFieldOptionDrafts,
      [fieldId]: name,
    };
  }
</script>

<section class="flex flex-col gap-0.5">
  <ProjectSettingsSectionHeading label={t("projects.customFields.title")} />
  <div class="flex flex-col gap-2">
    {#each projectCustomFields as field (field.id)}
      {@const fieldOptions = customFieldOptions(field)}
      {@const fieldOptionDraftRows = customFieldOptionCreateDraftRows(field.id)}
      {@const displayedFieldOptionCount = fieldOptions.length + fieldOptionDraftRows.length}
      <div class="custom-field-config flex flex-col gap-2">
        <div
          class={cn(
            "relative grid min-h-7 grid-cols-[auto_minmax(0,1fr)_auto_auto] items-center gap-1.5 px-1 py-0.5",
            draggedCustomFieldId === field.id && "opacity-50",
          )}
          role="group"
          aria-label={field.name}
          ondragover={(event) => onCustomFieldDragOver(event, field, event.currentTarget)}
          ondrop={(event) => { void onCustomFieldDrop(event, field); }}
        >
          {#if customFieldDropMarkerVisible(field.id, "before")}
            <div class="pointer-events-none absolute left-1 right-1 top-0 h-0.5 rounded-full bg-primary"></div>
          {/if}
          {#if customFieldDropMarkerVisible(field.id, "after")}
            <div class="pointer-events-none absolute bottom-0 left-1 right-1 h-0.5 rounded-full bg-primary"></div>
          {/if}
          <button
            type="button"
            class="flex h-7 w-7 shrink-0 cursor-grab items-center justify-center rounded-md text-muted-foreground hover:bg-accent hover:text-foreground active:cursor-grabbing disabled:cursor-not-allowed disabled:opacity-40"
            draggable={projectCustomFields.length > 1 && !customFieldReorderPending}
            disabled={projectCustomFields.length <= 1 || customFieldReorderPending}
            aria-label={t("projects.actions.dragCustomField", field.name)}
            ondragstart={(event) => onCustomFieldDragStart(event, field)}
            ondragend={clearCustomFieldDrag}
            onkeydown={(event) => {
              if (event.key === "ArrowUp") {
                event.preventDefault();
                void moveCustomFieldByDirection(field, -1);
              }
              if (event.key === "ArrowDown") {
                event.preventDefault();
                void moveCustomFieldByDirection(field, 1);
              }
            }}
          >
            <GripVertical size={13} strokeWidth={1.75} />
          </button>
          <input
            value={customFieldNameDraftValue(field)}
            class="field min-w-0 text-foreground"
            aria-label={t("projects.customFields.fieldName")}
            onpointerdown={moveTextInputCaretToPointer}
            oninput={(event) => setCustomFieldNameDraft(field.id, event.currentTarget.value)}
          />
          <div
            class="flex h-7 w-32 shrink-0 cursor-not-allowed items-center justify-between gap-2 rounded-md border border-border bg-transparent px-2.5 text-[0.8rem] font-medium text-foreground transition-colors"
            aria-label={t("projects.customFields.fieldType")}
            data-app-tooltip={t("projects.customFields.typeLockedTooltip")}
          >
            <span class="min-w-0 flex-1 truncate">{projectCustomFieldTypeLabel(field.fieldType, t)}</span>
            <ChevronDown size={13} strokeWidth={2} class="shrink-0 text-muted-foreground" />
          </div>
          <button
            type="button"
            class={projectSettingsIconButtonClass("danger")}
            aria-label={t("projects.actions.deleteCustomField", field.name)}
            title={t("projects.actions.deleteCustomField", field.name)}
            onclick={() => requestDeleteCustomField(field)}
          >
            <Trash2 size={13} strokeWidth={1.75} />
          </button>
        </div>

        {#if projectCustomFieldUsesOptions(field.fieldType)}
          <div class="flex flex-col gap-2 pl-9 pr-1">
            {#if displayedFieldOptionCount > 0}
              <div class="custom-field-option-branch flex flex-col gap-2">
                <svg
                  class="custom-field-option-connector"
                  aria-hidden="true"
                  viewBox={customFieldOptionConnectorViewBox(displayedFieldOptionCount)}
                  style={customFieldOptionConnectorStyle(displayedFieldOptionCount)}
                >
                  <path d={customFieldOptionConnectorPath(displayedFieldOptionCount)} />
                </svg>
                {#each fieldOptions as option (option.id)}
                  <div
                    class={cn(
                      "relative grid min-h-7 grid-cols-[auto_minmax(0,1fr)_auto] items-center gap-1.5 py-0.5",
                      draggedCustomFieldOptionId === option.id && "opacity-50",
                    )}
                    role="group"
                    aria-label={option.name}
                    ondragover={(event) => onCustomFieldOptionDragOver(event, option, event.currentTarget)}
                    ondrop={(event) => { void onCustomFieldOptionDrop(event, option); }}
                  >
                    {#if customFieldOptionDropMarkerVisible(option.id, "before")}
                      <div class="pointer-events-none absolute left-0 right-0 top-0 h-0.5 rounded-full bg-primary"></div>
                    {/if}
                    {#if customFieldOptionDropMarkerVisible(option.id, "after")}
                      <div class="pointer-events-none absolute bottom-0 left-0 right-0 h-0.5 rounded-full bg-primary"></div>
                    {/if}
                    <button
                      type="button"
                      class="flex h-7 w-7 shrink-0 cursor-grab items-center justify-center rounded-md text-muted-foreground hover:bg-accent hover:text-foreground active:cursor-grabbing disabled:cursor-not-allowed disabled:opacity-40"
                      draggable={fieldOptions.length > 1 && !customFieldOptionReorderPending}
                      disabled={fieldOptions.length <= 1 || customFieldOptionReorderPending}
                      aria-label={t("projects.actions.dragCustomFieldOption", option.name)}
                      ondragstart={(event) => onCustomFieldOptionDragStart(event, option)}
                      ondragend={clearCustomFieldOptionDrag}
                      onkeydown={(event) => {
                        if (event.key === "ArrowUp") {
                          event.preventDefault();
                          void moveCustomFieldOptionByDirection(option, -1);
                        }
                        if (event.key === "ArrowDown") {
                          event.preventDefault();
                          void moveCustomFieldOptionByDirection(option, 1);
                        }
                      }}
                    >
                      <GripVertical size={13} strokeWidth={1.75} />
                    </button>
                    <input
                      value={customFieldOptionNameDraftValue(option)}
                      class="field min-w-0 text-foreground"
                      aria-label={t("projects.customFields.optionName")}
                      onpointerdown={moveTextInputCaretToPointer}
                      oninput={(event) => setCustomFieldOptionNameDraft(option.id, event.currentTarget.value)}
                    />
                    <button
                      type="button"
                      class={projectSettingsIconButtonClass("danger")}
                      aria-label={t("projects.actions.deleteCustomFieldOption", option.name)}
                      title={t("projects.actions.deleteCustomFieldOption", option.name)}
                      onclick={() => requestDeleteCustomFieldOption(option)}
                    >
                      <Trash2 size={13} strokeWidth={1.75} />
                    </button>
                  </div>
                {/each}
                {#each fieldOptionDraftRows as option (option.id)}
                  <div
                    class="relative grid min-h-7 grid-cols-[auto_minmax(0,1fr)_auto] items-center gap-1.5 py-0.5"
                    role="group"
                    aria-label={option.name || t("projects.customFields.optionName")}
                  >
                    <div
                      class="flex h-7 w-7 shrink-0 cursor-not-allowed items-center justify-center rounded-md text-muted-foreground opacity-40"
                      aria-hidden="true"
                    >
                      <GripVertical size={13} strokeWidth={1.75} />
                    </div>
                    <input
                      value={option.name}
                      class="field min-w-0 text-foreground"
                      aria-label={t("projects.customFields.optionName")}
                      onpointerdown={moveTextInputCaretToPointer}
                      oninput={(event) => {
                        setCustomFieldOptionCreateDraftName(field.id, option.id, event.currentTarget.value);
                      }}
                    />
                    <button
                      type="button"
                      class={projectSettingsIconButtonClass("danger")}
                      aria-label={t("projects.actions.deleteCustomFieldOption", option.name)}
                      title={t("projects.actions.deleteCustomFieldOption", option.name)}
                      onclick={() => removeCustomFieldOptionCreateDraft(field.id, option.id)}
                    >
                      <Trash2 size={13} strokeWidth={1.75} />
                    </button>
                  </div>
                {/each}
              </div>
            {/if}
            <div class="grid min-h-7 grid-cols-[auto_minmax(0,1fr)_auto] items-center gap-1.5 py-0.5">
              <ProjectSettingsNewRowDragHandle showIcon={displayedFieldOptionCount === 0} />
              <input
                value={newCustomFieldOptionDrafts[field.id] ?? ""}
                class="field min-w-0 text-foreground"
                placeholder={t("projects.customFields.newOptionPlaceholder")}
                onpointerdown={moveTextInputCaretToPointer}
                oninput={(event) => setNewOptionDraft(field.id, event.currentTarget.value)}
                onkeydown={(event) => {
                  if (event.key === "Enter") {
                    event.preventDefault();
                    void submitCustomFieldOption(field);
                  }
                }}
              />
              <button
                type="button"
                class="flex h-7 w-7 shrink-0 items-center justify-center rounded-md bg-primary text-primary-foreground disabled:cursor-not-allowed disabled:opacity-50"
                aria-label={t("projects.customFields.addOption")}
                title={t("projects.customFields.addOption")}
                onclick={() => { void submitCustomFieldOption(field); }}
              >
                <Plus size={13} strokeWidth={1.75} />
              </button>
            </div>
          </div>
        {/if}
      </div>
    {/each}

    {#each customFieldCreateDraftRows as field (field.id)}
      {@const displayedFieldOptionCount = field.optionRows.length}
      <div class="custom-field-config flex flex-col gap-2">
        <div
          class="grid min-h-7 grid-cols-[auto_minmax(0,1fr)_auto_auto] items-center gap-1.5 px-1 py-0.5"
          role="group"
          aria-label={field.name || t("projects.customFields.fieldName")}
        >
          <div
            class="flex h-7 w-7 shrink-0 cursor-not-allowed items-center justify-center rounded-md text-muted-foreground opacity-40"
            aria-hidden="true"
          >
            <GripVertical size={13} strokeWidth={1.75} />
          </div>
          <input
            value={field.name}
            class="field min-w-0 text-foreground"
            aria-label={t("projects.customFields.fieldName")}
            onpointerdown={moveTextInputCaretToPointer}
            oninput={(event) => setCustomFieldCreateDraftName(field.id, event.currentTarget.value)}
          />
          <div
            class="flex h-7 w-32 shrink-0 cursor-not-allowed items-center justify-between gap-2 rounded-md border border-border bg-transparent px-2.5 text-[0.8rem] font-medium text-foreground transition-colors"
            aria-label={t("projects.customFields.fieldType")}
            data-app-tooltip={t("projects.customFields.typeLockedTooltip")}
          >
            <span class="min-w-0 flex-1 truncate">{projectCustomFieldTypeLabel(field.fieldType, t)}</span>
            <ChevronDown size={13} strokeWidth={2} class="shrink-0 text-muted-foreground" />
          </div>
          <button
            type="button"
            class={projectSettingsIconButtonClass("danger")}
            aria-label={t("projects.actions.deleteCustomField", field.name)}
            title={t("projects.actions.deleteCustomField", field.name)}
            onclick={() => removeCustomFieldCreateDraft(field.id)}
          >
            <Trash2 size={13} strokeWidth={1.75} />
          </button>
        </div>

        {#if projectCustomFieldUsesOptions(field.fieldType)}
          <div class="flex flex-col gap-2 pl-9 pr-1">
            {#if displayedFieldOptionCount > 0}
              <div class="custom-field-option-branch flex flex-col gap-2">
                <svg
                  class="custom-field-option-connector"
                  aria-hidden="true"
                  viewBox={customFieldOptionConnectorViewBox(displayedFieldOptionCount)}
                  style={customFieldOptionConnectorStyle(displayedFieldOptionCount)}
                >
                  <path d={customFieldOptionConnectorPath(displayedFieldOptionCount)} />
                </svg>
                {#each field.optionRows as option (option.id)}
                  <div
                    class="relative grid min-h-7 grid-cols-[auto_minmax(0,1fr)_auto] items-center gap-1.5 py-0.5"
                    role="group"
                    aria-label={option.name || t("projects.customFields.optionName")}
                  >
                    <div
                      class="flex h-7 w-7 shrink-0 cursor-not-allowed items-center justify-center rounded-md text-muted-foreground opacity-40"
                      aria-hidden="true"
                    >
                      <GripVertical size={13} strokeWidth={1.75} />
                    </div>
                    <input
                      value={option.name}
                      class="field min-w-0 text-foreground"
                      aria-label={t("projects.customFields.optionName")}
                      onpointerdown={moveTextInputCaretToPointer}
                      oninput={(event) =>
                        setCustomFieldCreateDraftOptionName(field.id, option.id, event.currentTarget.value)}
                    />
                    <button
                      type="button"
                      class={projectSettingsIconButtonClass("danger")}
                      aria-label={t("projects.actions.deleteCustomFieldOption", option.name)}
                      title={t("projects.actions.deleteCustomFieldOption", option.name)}
                      onclick={() => removeCustomFieldCreateDraftOption(field.id, option.id)}
                    >
                      <Trash2 size={13} strokeWidth={1.75} />
                    </button>
                  </div>
                {/each}
              </div>
            {/if}
            <div class="grid min-h-7 grid-cols-[auto_minmax(0,1fr)_auto] items-center gap-1.5 py-0.5">
              <ProjectSettingsNewRowDragHandle showIcon={displayedFieldOptionCount === 0} />
              <input
                value={field.optionName}
                class="field min-w-0 text-foreground"
                placeholder={t("projects.customFields.newOptionPlaceholder")}
                onpointerdown={moveTextInputCaretToPointer}
                oninput={(event) =>
                  setCustomFieldCreateDraftPendingOptionName(field.id, event.currentTarget.value)}
                onkeydown={(event) => {
                  if (event.key === "Enter") {
                    event.preventDefault();
                    submitCustomFieldCreateDraftOption(field);
                  }
                }}
              />
              <button
                type="button"
                class="flex h-7 w-7 shrink-0 items-center justify-center rounded-md bg-primary text-primary-foreground disabled:cursor-not-allowed disabled:opacity-50"
                aria-label={t("projects.customFields.addOption")}
                title={t("projects.customFields.addOption")}
                onclick={() => submitCustomFieldCreateDraftOption(field)}
              >
                <Plus size={13} strokeWidth={1.75} />
              </button>
            </div>
          </div>
        {/if}
      </div>
    {/each}

    <div class="flex flex-col gap-2">
      <div
        bind:this={newCustomFieldRowElement}
        class="grid min-h-7 grid-cols-[auto_minmax(0,1fr)_auto_auto] items-center gap-1.5 px-1 py-0.5"
      >
        <ProjectSettingsNewRowDragHandle
          showIcon={projectCustomFields.length + customFieldCreateDraftRows.length === 0}
        />
        <input
          bind:value={newCustomFieldName}
          class="field min-w-0 text-foreground"
          placeholder={t("projects.customFields.newFieldPlaceholder")}
          onpointerdown={moveTextInputCaretToPointer}
          onkeydown={(event) => {
            if (event.key === "Enter") {
              event.preventDefault();
              submitCustomField();
            }
          }}
        />
        <ProjectSettingsCustomFieldTypePicker
          bind:value={newCustomFieldType}
          containerClass="w-32"
        />
        <button
          type="button"
          class="flex h-7 w-7 shrink-0 items-center justify-center rounded-md bg-primary text-primary-foreground disabled:cursor-not-allowed disabled:opacity-50"
          aria-label={t("projects.customFields.addField")}
          title={t("projects.customFields.addField")}
          onclick={submitCustomField}
        >
          <Plus size={13} strokeWidth={1.75} />
        </button>
      </div>

      {#if projectCustomFieldUsesOptions(newCustomFieldType)}
        <div class="flex flex-col gap-2 pl-9 pr-1">
          {#each newCustomFieldOptionRows as option (option.id)}
            <div class="grid min-h-7 grid-cols-[auto_minmax(0,1fr)_auto] items-center gap-1.5 py-0.5">
              <div
                class="flex h-7 w-7 shrink-0 items-center justify-center rounded-md text-muted-foreground opacity-40"
                aria-hidden="true"
              >
                <GripVertical size={13} strokeWidth={1.75} />
              </div>
              <input
                value={option.name}
                class="field min-w-0 text-foreground"
                aria-label={t("projects.customFields.optionName")}
                onpointerdown={moveTextInputCaretToPointer}
                oninput={(event) => setNewCustomFieldOptionDraftName(option.id, event.currentTarget.value)}
              />
              <button
                type="button"
                class={projectSettingsIconButtonClass("danger")}
                aria-label={t("projects.actions.deleteCustomFieldOption", option.name)}
                title={t("projects.actions.deleteCustomFieldOption", option.name)}
                onclick={() => removeNewCustomFieldOptionDraft(option.id)}
              >
                <Trash2 size={13} strokeWidth={1.75} />
              </button>
            </div>
          {/each}
          <div class="grid min-h-7 grid-cols-[auto_minmax(0,1fr)_auto] items-center gap-1.5 py-0.5">
            <ProjectSettingsNewRowDragHandle showIcon={newCustomFieldOptionRows.length === 0} />
            <input
              bind:value={newCustomFieldOptionName}
              class="field min-w-0 text-foreground"
              placeholder={t("projects.customFields.newOptionPlaceholder")}
              onpointerdown={moveTextInputCaretToPointer}
              onkeydown={(event) => {
                if (event.key === "Enter") {
                  event.preventDefault();
                  submitNewCustomFieldOptionDraft();
                }
              }}
            />
            <button
              type="button"
              class="flex h-7 w-7 shrink-0 items-center justify-center rounded-md bg-primary text-primary-foreground disabled:cursor-not-allowed disabled:opacity-50"
              aria-label={t("projects.customFields.addOption")}
              title={t("projects.customFields.addOption")}
              onclick={submitNewCustomFieldOptionDraft}
            >
              <Plus size={13} strokeWidth={1.75} />
            </button>
          </div>
        </div>
      {/if}
    </div>
  </div>
</section>

<style>
  .custom-field-config {
    --custom-field-option-branch-color: color-mix(
      in srgb,
      var(--muted-foreground) 54%,
      var(--card)
    );
  }

  .custom-field-option-branch {
    position: relative;
  }

  .custom-field-option-connector {
    position: absolute;
    pointer-events: none;
    overflow: visible;
  }

  .custom-field-option-connector path {
    fill: none;
    stroke: var(--custom-field-option-branch-color);
    stroke-linecap: round;
    stroke-linejoin: round;
    stroke-width: 0.125;
  }
</style>
