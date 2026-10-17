<script lang="ts">
  import Select from "$lib/components/ui/Select.svelte";
  import X from "@lucide/svelte/icons/x";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import {
    projectTagColorDotStyle,
    projectTagColorSwatchClass,
  } from "$lib/projects/display";
  import type { ProjectTag, ProjectTask } from "$lib/projects/types";
  import type { Theme } from "$lib/themes";
  import { cn, type MaybePromise } from "$lib/utils";
  import ProjectSettingsSectionHeading from "$lib/components/projects/settings/ProjectSettingsSectionHeading.svelte";


  let {
    task,
    tags,
    candidates,
    draft,
    canCreate,
    theme,
    onDraftChange,
    onAttachTag,
    onSubmitTag,
    onDetachTag,
  }: {
    task: ProjectTask;
    tags: ProjectTag[];
    candidates: ProjectTag[];
    draft: string;
    canCreate: boolean;
    theme: Theme;
    onDraftChange: (value: string) => void;
    onAttachTag: (task: ProjectTask, tag: ProjectTag) => MaybePromise;
    onSubmitTag: (task: ProjectTask) => MaybePromise;
    onDetachTag: (task: ProjectTask, tag: ProjectTag) => MaybePromise;
  } = $props();

  const { t } = getLocalization();
  const CREATE_TAG_OPTION = "create-tag";
</script>

<section class="task-detail-section grid min-w-0 content-start gap-3">
  <ProjectSettingsSectionHeading label={t("projects.detail.tags")} count={tags.length} inlineCount />
  {#if tags.length > 0}
    <div class="flex flex-wrap gap-1">
      {#each tags as tag (tag.id)}
        <span class="inline-flex min-h-7 max-w-full items-center gap-1 rounded-md bg-muted/60 px-2 text-[0.766667rem]">
          <span
            class={cn("h-2 w-2 shrink-0 rounded-full border", projectTagColorSwatchClass(tag.color))}
            style={projectTagColorDotStyle(tag.color, theme)}
          ></span>
          <span class="truncate">{tag.name}</span>
          <button
            type="button"
            class="flex h-5 w-5 shrink-0 items-center justify-center rounded text-muted-foreground hover:bg-destructive/10 hover:text-destructive"
            aria-label={t("projects.actions.removeTag", tag.name)}
            title={t("projects.actions.removeTag", tag.name)}
            onclick={() => { void onDetachTag(task, tag); }}
          >
            <X size={12} strokeWidth={1.75} />
          </button>
        </span>
      {/each}
    </div>
  {/if}
  <Select inline appearance="quiet" contentAlign="start" class="w-full" value=""
    triggerLabel={t("projects.detail.addTag")} ariaLabel={t("projects.detail.addTag")}
    options={[
      ...candidates.map((tag) => ({ value: tag.id, label: tag.name })),
      ...(canCreate ? [{ value: CREATE_TAG_OPTION, label: t("projects.detail.createTag", draft.trim()) }] : []),
    ]}
    searchPlaceholder={t("projects.detail.addTagPlaceholder")}
    searchValue={draft} onSearchChange={onDraftChange}
    emptyLabel={t("projects.detail.noTagCandidates")}
    onChange={(value) => {
      if (value === CREATE_TAG_OPTION) { void onSubmitTag(task); return; }
      const tag = candidates.find((candidate) => candidate.id === value);
      if (tag) void onAttachTag(task, tag);
    }}>
    {#snippet leading(value)}
      {@const tag = candidates.find((candidate) => candidate.id === value)}
      {#if tag}<span class="size-2 shrink-0 rounded-full" style={projectTagColorDotStyle(tag.color, theme)}></span>{/if}
    {/snippet}
  </Select>
</section>
