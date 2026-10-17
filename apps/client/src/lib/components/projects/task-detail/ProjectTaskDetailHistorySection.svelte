<script lang="ts">
  import { formatDateTime } from "$lib/i18n/formatters";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import { projectTaskHistoryEventLabel } from "$lib/projects/display";
  import type { ProjectTaskChangeEvent } from "$lib/projects/types";
  import ProjectSettingsSectionHeading from "$lib/components/projects/settings/ProjectSettingsSectionHeading.svelte";

  let {
    events,
  }: {
    events: ProjectTaskChangeEvent[];
  } = $props();

  const localization = getLocalization();
  const { t } = localization;
</script>

<section class="task-detail-section grid min-w-0 content-start gap-3">
  <ProjectSettingsSectionHeading label={t("projects.detail.history")} count={events.length} inlineCount />
    <div class="grid gap-1">
      {#each events as event (event.id)}
        <div class="grid gap-1 border-l-2 border-border/70 py-1 pl-3">
          <div class="truncate text-[0.8rem]">{projectTaskHistoryEventLabel(event, t)}</div>
          {#if event.reason}
            <div class="truncate text-[0.733333rem] text-muted-foreground">
              {t("projects.history.reason", event.reason)}
            </div>
          {/if}
          <div class="truncate text-[0.733333rem] text-muted-foreground">{formatDateTime(localization.locale, new Date(event.occurredAt), { dateStyle: "medium", timeStyle: "short" })}</div>
        </div>
      {:else}
        <div class="px-1 py-1 text-[0.8rem] text-muted-foreground">
          {t("projects.detail.noHistory")}
        </div>
      {/each}
    </div>
</section>
