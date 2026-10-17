<script lang="ts">
  import ArrowDown from "@lucide/svelte/icons/arrow-down";
  import ArrowUp from "@lucide/svelte/icons/arrow-up";
  import Plus from "@lucide/svelte/icons/plus";
  import Save from "@lucide/svelte/icons/save";
  import Trash2 from "@lucide/svelte/icons/trash-2";
  import Checkbox from "$lib/components/ui/Checkbox.svelte";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import type { MoveDirection, ProjectChecklistItem, ProjectTask } from "$lib/projects/types";
  import { cn, type MaybePromise } from "$lib/utils";
  import ProjectSettingsSectionHeading from "$lib/components/projects/settings/ProjectSettingsSectionHeading.svelte";


  let {
    task,
    items,
    titleDrafts,
    draft,
    onTitleDraftChange,
    onDraftChange,
    onToggleCompleted,
    onSaveItem,
    onMoveItem,
    onDeleteItem,
    onSubmitItem,
  }: {
    task: ProjectTask;
    items: ProjectChecklistItem[];
    titleDrafts: Record<string, string>;
    draft: string;
    onTitleDraftChange: (itemId: string, value: string) => void;
    onDraftChange: (value: string) => void;
    onToggleCompleted: (item: ProjectChecklistItem, completed: boolean) => MaybePromise;
    onSaveItem: (item: ProjectChecklistItem) => MaybePromise;
    onMoveItem: (item: ProjectChecklistItem, direction: MoveDirection) => MaybePromise;
    onDeleteItem: (item: ProjectChecklistItem) => MaybePromise;
    onSubmitItem: (task: ProjectTask) => MaybePromise;
  } = $props();

  const { t } = getLocalization();

  function checklistItemDraftTitle(item: ProjectChecklistItem): string {
    return titleDrafts[item.id] ?? item.title;
  }

  function checklistItemDirty(item: ProjectChecklistItem): boolean {
    return checklistItemDraftTitle(item) !== item.title;
  }

  function adjacentChecklistItem(
    item: ProjectChecklistItem,
    direction: MoveDirection,
  ): ProjectChecklistItem | undefined {
    const index = items.findIndex((entry) => entry.id === item.id);
    if (index < 0) return undefined;
    return items[index + direction];
  }
</script>

<section class="task-detail-section grid min-w-0 content-start gap-3">
  <ProjectSettingsSectionHeading label={t("projects.detail.checklist")} count={items.length} inlineCount />
  <div class="grid gap-1">
    {#each items as item (item.id)}
      {@const previousChecklistItem = adjacentChecklistItem(item, -1)}
      {@const nextChecklistItem = adjacentChecklistItem(item, 1)}
      <div class="grid min-h-8 grid-cols-[auto_minmax(0,1fr)_auto_auto_auto_auto] items-center gap-1 rounded-md bg-transparent px-2 hover:bg-muted/40">
        <Checkbox
          checked={Boolean(item.completedAt)}
          label={t("projects.actions.toggleChecklistItem")}
          onChange={(next) => { void onToggleCompleted(item, next); }}
        />
        <input
          value={checklistItemDraftTitle(item)}
          class={cn(
            "min-h-7 min-w-0 bg-transparent px-1 text-[0.8rem]",
            item.completedAt ? "text-muted-foreground line-through" : "text-foreground",
          )}
          aria-label={t("projects.detail.checklistItemTitle")}
          oninput={(event) => onTitleDraftChange(item.id, event.currentTarget.value)}
          onkeydown={(event) => {
            if (event.key === "Enter") {
              event.preventDefault();
              void onSaveItem(item);
            }
          }}
        />
        <button
          type="button"
          class="flex h-6 w-6 shrink-0 items-center justify-center rounded text-muted-foreground hover:bg-accent hover:text-foreground disabled:cursor-not-allowed disabled:opacity-40"
          disabled={!previousChecklistItem}
          aria-label={previousChecklistItem ? t("projects.actions.moveChecklistItemUp", item.title) : t("projects.actions.noPreviousChecklistItem")}
          title={previousChecklistItem ? t("projects.actions.moveChecklistItemUp", item.title) : t("projects.actions.noPreviousChecklistItem")}
          onclick={() => { void onMoveItem(item, -1); }}
        >
          <ArrowUp size={13} strokeWidth={1.75} />
        </button>
        <button
          type="button"
          class="flex h-6 w-6 shrink-0 items-center justify-center rounded text-muted-foreground hover:bg-accent hover:text-foreground disabled:cursor-not-allowed disabled:opacity-40"
          disabled={!nextChecklistItem}
          aria-label={nextChecklistItem ? t("projects.actions.moveChecklistItemDown", item.title) : t("projects.actions.noNextChecklistItem")}
          title={nextChecklistItem ? t("projects.actions.moveChecklistItemDown", item.title) : t("projects.actions.noNextChecklistItem")}
          onclick={() => { void onMoveItem(item, 1); }}
        >
          <ArrowDown size={13} strokeWidth={1.75} />
        </button>
        <button
          type="button"
          class="flex h-6 w-6 shrink-0 items-center justify-center rounded text-muted-foreground hover:bg-accent hover:text-foreground disabled:cursor-not-allowed disabled:opacity-40"
          disabled={!checklistItemDirty(item)}
          aria-label={t("projects.actions.saveChecklistItem", item.title)}
          title={t("projects.actions.saveChecklistItem", item.title)}
          onclick={() => { void onSaveItem(item); }}
        >
          <Save size={13} strokeWidth={1.75} />
        </button>
        <button
          type="button"
          class="flex h-6 w-6 shrink-0 items-center justify-center rounded text-muted-foreground hover:bg-destructive/10 hover:text-destructive"
          aria-label={t("projects.actions.deleteChecklistItem", item.title)}
          onclick={() => { void onDeleteItem(item); }}
        >
          <Trash2 size={13} strokeWidth={1.75} />
        </button>
      </div>
    {/each}
  </div>
  <div class="flex gap-1">
    <input
      value={draft}
      aria-label={t("projects.detail.addChecklistItem")}
      placeholder={t("projects.detail.addChecklistItemPlaceholder")}
      class="min-h-8 min-w-0 flex-1 rounded-md bg-transparent px-2 hover:bg-muted/40 text-[0.8rem]"
      oninput={(event) => onDraftChange(event.currentTarget.value)}
      onkeydown={(event) => {
        if (event.key === "Enter") {
          event.preventDefault();
          void onSubmitItem(task);
        }
      }}
    />
    <button
      type="button"
      class="flex min-h-8 items-center justify-center rounded-md bg-transparent px-2 text-[0.8rem] hover:bg-accent"
      aria-label={t("projects.detail.addChecklistItem")}
      onclick={() => { void onSubmitItem(task); }}
    >
      <Plus size={14} strokeWidth={1.75} />
    </button>
  </div>
</section>
