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
  import type { MoveDirection, ProjectTag } from "$lib/projects/types";
  import type { Theme } from "$lib/themes";
  import { cn, type MaybePromise } from "$lib/utils";
  import ProjectSettingsNewRowDragHandle from "./ProjectSettingsNewRowDragHandle.svelte";
  import ProjectSettingsSectionHeading from "./ProjectSettingsSectionHeading.svelte";


  let {
    theme,
    tags,
    draggedTagId,
    tagReorderPending,
    newTagRowElement = $bindable<HTMLDivElement | undefined>(),
    newTagName = $bindable<string>(),
    newTagColor = $bindable<EventColor>(),
    tagDropMarkerVisible,
    onTagDragOver,
    onTagDrop,
    onTagDragStart,
    clearTagDrag,
    moveTagByDirection,
    tagColorDraftValue,
    setTagColor,
    tagNameDraftValue,
    setTagNameDraft,
    requestDeleteTag,
    setNewTagColor,
    submitTag,
  }: {
    theme: Theme;
    tags: ProjectTag[];
    draggedTagId: string | null;
    tagReorderPending: boolean;
    newTagRowElement: HTMLDivElement | undefined;
    newTagName: string;
    newTagColor: EventColor;
    tagDropMarkerVisible: (tagId: string, position: ProjectSettingsDropPosition) => boolean;
    onTagDragOver: (event: DragEvent, tag: ProjectTag, target: HTMLElement) => void;
    onTagDrop: (event: DragEvent, tag: ProjectTag) => MaybePromise;
    onTagDragStart: (event: DragEvent, tag: ProjectTag) => void;
    clearTagDrag: () => void;
    moveTagByDirection: (tag: ProjectTag, direction: MoveDirection) => MaybePromise;
    tagColorDraftValue: (tag: ProjectTag) => EventColor;
    setTagColor: (tagId: string, color: EventColor | undefined) => void;
    tagNameDraftValue: (tag: ProjectTag) => string;
    setTagNameDraft: (tagId: string, name: string) => void;
    requestDeleteTag: (tag: ProjectTag) => void;
    setNewTagColor: (color: EventColor | undefined) => void;
    submitTag: () => MaybePromise;
  } = $props();

  const { t } = getLocalization();
</script>

<section class="flex flex-col gap-0.5">
  <ProjectSettingsSectionHeading label={t("projects.settings.tags")} />
  <div class="flex flex-col gap-2">
    {#each tags as tag (tag.id)}
      <div
        class={cn(
          "relative grid min-h-7 grid-cols-[auto_auto_minmax(0,1fr)_auto] items-center gap-1.5 px-1 py-0.5",
          draggedTagId === tag.id && "opacity-50",
        )}
        role="group"
        aria-label={tag.name}
        ondragover={(event) => onTagDragOver(event, tag, event.currentTarget)}
        ondrop={(event) => { void onTagDrop(event, tag); }}
      >
        {#if tagDropMarkerVisible(tag.id, "before")}
          <div class="pointer-events-none absolute left-1 right-1 top-0 h-0.5 rounded-full bg-primary"></div>
        {/if}
        {#if tagDropMarkerVisible(tag.id, "after")}
          <div class="pointer-events-none absolute bottom-0 left-1 right-1 h-0.5 rounded-full bg-primary"></div>
        {/if}
        <button
          type="button"
          class="flex h-7 w-7 shrink-0 cursor-grab items-center justify-center rounded-md text-muted-foreground hover:bg-accent hover:text-foreground active:cursor-grabbing disabled:cursor-not-allowed disabled:opacity-40"
          draggable={tags.length > 1 && !tagReorderPending}
          disabled={tags.length <= 1 || tagReorderPending}
          aria-label={t("projects.actions.dragTag", tag.name)}
          ondragstart={(event) => onTagDragStart(event, tag)}
          ondragend={clearTagDrag}
          onkeydown={(event) => {
            if (event.key === "ArrowUp") {
              event.preventDefault();
              void moveTagByDirection(tag, -1);
            }
            if (event.key === "ArrowDown") {
              event.preventDefault();
              void moveTagByDirection(tag, 1);
            }
          }}
        >
          <GripVertical size={13} strokeWidth={1.75} />
        </button>
        <EventColorPicker
          color={tagColorDraftValue(tag)}
          {theme}
          ariaLabel={t("projects.settings.selectTagColor", tag.name)}
          class="h-7 w-7 justify-center self-center"
          buttonClass="size-6 rounded-md"
          onSelect={(color) => setTagColor(tag.id, color)}
        />
        <input
          value={tagNameDraftValue(tag)}
          class="field min-w-0 text-foreground"
          aria-label={t("projects.settings.tagName")}
          onpointerdown={moveTextInputCaretToPointer}
          oninput={(event) => setTagNameDraft(tag.id, event.currentTarget.value)}
        />
        <button
          type="button"
          class={projectSettingsIconButtonClass("danger")}
          aria-label={t("projects.actions.deleteTag", tag.name)}
          title={t("projects.actions.deleteTag", tag.name)}
          onclick={() => requestDeleteTag(tag)}
        >
          <Trash2 size={13} strokeWidth={1.75} />
        </button>
      </div>
    {/each}

    <div
      bind:this={newTagRowElement}
      class="grid min-h-7 grid-cols-[auto_auto_minmax(0,1fr)_auto] items-center gap-1.5 px-1 py-0.5"
    >
      <ProjectSettingsNewRowDragHandle showIcon={tags.length === 0} />
      <EventColorPicker
        color={newTagColor}
        {theme}
        ariaLabel={t("projects.settings.selectNewTagColor")}
        class="h-7 w-7 justify-center self-center"
        buttonClass="size-6 rounded-md"
        onSelect={setNewTagColor}
      />
      <input
        bind:value={newTagName}
        class="field min-w-0 text-foreground"
        placeholder={t("projects.settings.newTagPlaceholder")}
        onpointerdown={moveTextInputCaretToPointer}
        onkeydown={(event) => {
          if (event.key === "Enter") {
            event.preventDefault();
            void submitTag();
          }
        }}
      />
      <button
        type="button"
        class="flex h-7 w-7 shrink-0 items-center justify-center rounded-md bg-primary text-primary-foreground disabled:cursor-not-allowed disabled:opacity-50"
        aria-label={t("projects.settings.addTag")}
        title={t("projects.settings.addTag")}
        onclick={() => { void submitTag(); }}
      >
        <Plus size={13} strokeWidth={1.75} />
      </button>
    </div>
  </div>
</section>
