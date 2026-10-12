<script lang="ts">
  import Checkbox from "$lib/components/ui/Checkbox.svelte";
  import Select from "$lib/components/ui/Select.svelte";
  import Check from "@lucide/svelte/icons/check";
  import Save from "@lucide/svelte/icons/save";
  import {
    projectCustomFieldInputType,
    projectCustomFieldTextInputMode,
    projectCustomFieldUsesOptions,
    projectCustomFieldUsesTextValue,
  } from "$lib/projects/custom-fields";
  import { projectCustomFieldTypeLabel } from "$lib/projects/display";
  import type {
    ProjectCustomField,
    ProjectCustomFieldOption,
    ProjectTask,
  } from "$lib/projects/types";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import { cn, type MaybePromise } from "$lib/utils";
  import ProjectTaskDetailDateField from "./ProjectTaskDetailDateField.svelte";
  import ProjectSettingsSectionHeading from "$lib/components/projects/settings/ProjectSettingsSectionHeading.svelte";


  let {
    task,
    fields,
    textDrafts,
    numberDrafts,
    dateDrafts,
    checkboxDrafts,
    selectDrafts,
    multiDrafts,
    datePickerTarget,
    todayDate,
    customFieldOptions,
    customFieldValueDirty,
    onTextDraftChange,
    onNumberDraftChange,
    onCheckboxDraftChange,
    onSelectDraftChange,
    onToggleMultiOption,
    onSaveField,
    onToggleDatePicker,
    onClearDate,
    onSelectDate,
    onCancelDatePicker,
  }: {
    task: ProjectTask;
    fields: ProjectCustomField[];
    textDrafts: Record<string, string>;
    numberDrafts: Record<string, string>;
    dateDrafts: Record<string, string>;
    checkboxDrafts: Record<string, boolean>;
    selectDrafts: Record<string, string>;
    multiDrafts: Record<string, string[]>;
    datePickerTarget: string | null;
    todayDate: string;
    customFieldOptions: (field: ProjectCustomField) => ProjectCustomFieldOption[];
    customFieldValueDirty: (task: ProjectTask, field: ProjectCustomField) => boolean;
    onTextDraftChange: (fieldId: string, value: string) => void;
    onNumberDraftChange: (fieldId: string, value: string) => void;
    onCheckboxDraftChange: (fieldId: string, value: boolean) => void;
    onSelectDraftChange: (fieldId: string, value: string) => void;
    onToggleMultiOption: (field: ProjectCustomField, option: ProjectCustomFieldOption) => void;
    onSaveField: (task: ProjectTask, field: ProjectCustomField) => MaybePromise;
    onToggleDatePicker: (fieldId: string) => void;
    onClearDate: (fieldId: string) => void;
    onSelectDate: (date: string) => void;
    onCancelDatePicker: () => void;
  } = $props();

  const { t } = getLocalization();
</script>

<section class="task-detail-section grid min-w-0 content-start gap-3">
  <ProjectSettingsSectionHeading label={t("projects.customFields.taskValues")} count={fields.length} inlineCount />
  <div class="grid gap-2">
    {#each fields as field (field.id)}
      <div class="grid gap-1 border-b border-border/50 py-2 last:border-b-0">
        <div class="flex min-w-0 items-center justify-between gap-2">
          <div class="min-w-0">
            <div class="truncate text-[0.8rem] font-medium">{field.name}</div>
            <div class="truncate text-[0.733333rem] text-muted-foreground">
              {projectCustomFieldTypeLabel(field.fieldType, t)}
            </div>
          </div>
          <button
            type="button"
            class="flex min-h-7 shrink-0 items-center gap-1 rounded-md border border-border bg-card px-2 text-[0.733333rem] hover:bg-accent disabled:cursor-not-allowed disabled:opacity-50"
            disabled={!customFieldValueDirty(task, field)}
            onclick={() => { void onSaveField(task, field); }}
          >
            <Save size={13} strokeWidth={1.75} />
            <span>{t("projects.customFields.saveValue")}</span>
          </button>
        </div>

        {#if projectCustomFieldUsesTextValue(field.fieldType)}
          <input
            type={projectCustomFieldInputType(field.fieldType)}
            value={textDrafts[field.id] ?? ""}
            inputmode={projectCustomFieldTextInputMode(field.fieldType)}
            class="min-h-8 min-w-0 rounded-md bg-transparent px-2 hover:bg-muted/40 text-[0.8rem] text-foreground"
            aria-label={field.name}
            placeholder={t("projects.customFields.emptyValue")}
            oninput={(event) => onTextDraftChange(field.id, event.currentTarget.value)}
            onkeydown={(event) => {
              if (event.key === "Enter") {
                event.preventDefault();
                void onSaveField(task, field);
              }
            }}
          />
        {:else if field.fieldType === "number"}
          <input
            value={numberDrafts[field.id] ?? ""}
            inputmode="decimal"
            class="min-h-8 min-w-0 rounded-md bg-transparent px-2 hover:bg-muted/40 text-[0.8rem] text-foreground"
            aria-label={field.name}
            placeholder={t("projects.customFields.emptyValue")}
            oninput={(event) => onNumberDraftChange(field.id, event.currentTarget.value)}
            onkeydown={(event) => {
              if (event.key === "Enter") {
                event.preventDefault();
                void onSaveField(task, field);
              }
            }}
          />
        {:else if field.fieldType === "date"}
          <ProjectTaskDetailDateField
            label={field.name}
            value={dateDrafts[field.id] ?? ""}
            noDateLabel={t("projects.detail.noDate")}
            clearLabel={t("projects.detail.clearDate", field.name)}
            pickerOpen={datePickerTarget === field.id}
            selectedDate={dateDrafts[field.id] || todayDate}
            highlightMode="none"
            onToggle={() => onToggleDatePicker(field.id)}
            onClear={() => onClearDate(field.id)}
            onSelect={onSelectDate}
            onCancel={onCancelDatePicker}
          />
        {:else if field.fieldType === "checkbox"}
          <label class="flex min-h-8 items-center gap-2 rounded-floating-item border border-border px-2 text-[0.8rem]">
            <Checkbox
              checked={checkboxDrafts[field.id] ?? false}
              onChange={(next) => onCheckboxDraftChange(field.id, next)}
            />
            <span>{field.name}</span>
          </label>
        {:else if field.fieldType === "select" || field.fieldType === "status"}
          <Select inline appearance="quiet" contentAlign="start" class="w-full"
            value={selectDrafts[field.id] ?? "none"} ariaLabel={field.name}
            options={[
              { value: "none", label: t("projects.customFields.selectNone") },
              ...customFieldOptions(field).map((option) => ({ value: option.id, label: option.name })),
            ]}
            onChange={(value) => onSelectDraftChange(field.id, value)} />
        {:else if projectCustomFieldUsesOptions(field.fieldType)}
          <div class="flex flex-wrap gap-1">
            {#each customFieldOptions(field) as option (option.id)}
              {@const optionSelected = (multiDrafts[field.id] ?? []).includes(option.id)}
              <button
                type="button"
                class={cn(
                  "flex min-h-7 items-center gap-1 rounded-md border px-2 text-[0.733333rem]",
                  optionSelected
                    ? "border-primary/50 bg-primary/10 text-primary"
                    : "border-border bg-card text-muted-foreground hover:bg-accent hover:text-foreground",
                )}
                onclick={() => onToggleMultiOption(field, option)}
              >
                {#if optionSelected}
                  <Check size={12} strokeWidth={1.75} />
                {/if}
                <span>{option.name}</span>
              </button>
            {:else}
              <div class="rounded-md border border-dashed border-border px-2 py-2 text-[0.8rem] text-muted-foreground">
                {t("projects.customFields.noOptions")}
              </div>
            {/each}
          </div>
        {/if}
      </div>
    {/each}
  </div>
</section>
