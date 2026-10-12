<script lang="ts">
  import ArrowUp from "@lucide/svelte/icons/arrow-up";
  import Plus from "@lucide/svelte/icons/plus";
  import Trash2 from "@lucide/svelte/icons/trash-2";
  import CollectionMenu from "$lib/components/collections/CollectionMenu.svelte";
  import Notice from "$lib/components/ui/Notice.svelte";
  import Select from "$lib/components/ui/Select.svelte";
  import { EVENT_COLOR_OPTIONS, getEventColor } from "$lib/calendar/utils";
  import { FALLBACK_COLOR_INDEX } from "$lib/calendar/types";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import { getTheme } from "$lib/stores/theme.svelte";
  import type { ProjectRowColorRule } from "$lib/projects/list/presentation";
  import type { ProjectTaskQueryController } from "$lib/components/projects/task-query-controller.svelte";

  let { query }: { query: ProjectTaskQueryController } = $props();
  const { t } = getLocalization();
  const theme = getTheme();

  /** Persist rule order because the first matching condition determines each row's color. */
  function saveRules(colorRules: ProjectRowColorRule[]): void {
    void query.savePresentation({ ...query.listPresentation, colorRules });
  }

  /** Update one rule without replacing the stable identities of the other conditions. */
  function updateRule(rule: ProjectRowColorRule, patch: Partial<Omit<ProjectRowColorRule, "id">>): void {
    saveRules(query.listPresentation.colorRules.map((entry) => entry.id === rule.id ? { ...entry, ...patch } : entry));
  }

  /** Add a usable status condition from this project's current schema. */
  function addRule(): void {
    const value = query.statuses[0]?.id;
    if (!value) return;
    saveRules([...query.listPresentation.colorRules, { id: crypto.randomUUID(), property: "status", value, color: FALLBACK_COLOR_INDEX }]);
  }

  /** Move a condition ahead of the preceding condition without changing its predicate. */
  function moveUp(index: number): void {
    if (index <= 0) return;
    const rules = [...query.listPresentation.colorRules];
    [rules[index - 1], rules[index]] = [rules[index], rules[index - 1]];
    saveRules(rules);
  }
</script>

<CollectionMenu label={t("projects.columns.rowColors")} kind="layout" fullWidth summary={query.listPresentation.colorRules.length ? t("projects.columns.ruleCount", query.listPresentation.colorRules.length) : t("projects.toolbar.none")}>
  <p class="px-2 pb-2 text-panel-detail text-muted-foreground">{t("projects.columns.firstMatchingColor")}</p>
  {#each query.listPresentation.colorRules as rule, index (rule.id)}
    <div class="mb-2 grid min-w-0 gap-1 rounded-floating-item border border-border p-2">
      <div class="flex gap-1">
        <Select textSize="collection" inline appearance="quiet" ariaLabel={t("projects.columns.colorProperty")} value={rule.property} disabled={query.presentationSaving}
          options={[{value: "status", label: t("projects.columns.status")}, {value: "priority", label: t("projects.columns.priority")}]}
          onChange={(value) => {
            if (value === "status" || value === "priority") {
              const first = (value === "status" ? query.statuses : query.priorities)[0];
              if (first) updateRule(rule, { property: value, value: first.id });
            }
          }} />
        <button type="button" class="flex size-(--panel-row-height) shrink-0 items-center justify-center rounded-floating-item hover:bg-accent" disabled={query.presentationSaving || index === 0} aria-label={t("projects.columns.moveRuleUp")} onclick={() => moveUp(index)}><ArrowUp class="size-3.5" /></button>
        <button type="button" class="flex size-(--panel-row-height) shrink-0 items-center justify-center rounded-floating-item hover:bg-accent" disabled={query.presentationSaving} aria-label={t("projects.columns.removeRule")} onclick={() => saveRules(query.listPresentation.colorRules.filter((entry) => entry.id !== rule.id))}><Trash2 class="size-3.5" /></button>
      </div>
      <Select textSize="collection" inline appearance="quiet" ariaLabel={t("projects.columns.colorValue")} value={rule.value} disabled={query.presentationSaving}
        options={(rule.property === "status" ? query.statuses : query.priorities).map((entry) => ({ value: entry.id, label: entry.name }))}
        onChange={(value) => updateRule(rule, { value })} />
      <CollectionMenu label={t("projects.columns.rowColor")} kind="layout" fullWidth disabled={query.presentationSaving}>
        <div class="grid grid-cols-4 gap-1">
          {#each EVENT_COLOR_OPTIONS as color}
            <button type="button" class="min-h-(--panel-row-height) rounded-floating-item border border-border" style={`background-color: ${getEventColor(color, theme.current).bg};`}
              aria-label={t("calendar.color.selectEventColor", color + 1)} aria-pressed={rule.color === color} disabled={query.presentationSaving} onclick={() => updateRule(rule, { color })}></button>
          {/each}
        </div>
      </CollectionMenu>
    </div>
  {/each}
  <button type="button" class="menu-item" disabled={query.presentationSaving || query.statuses.length === 0 || query.listPresentation.colorRules.length >= 32} onclick={addRule}><Plus class="size-3.5" />{t("projects.columns.addColorRule")}</button>
  {#if query.presentationError}<Notice class="px-2 py-1" message={query.presentationError} />{/if}
</CollectionMenu>
