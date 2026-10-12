<script lang="ts">
  import Check from "@lucide/svelte/icons/check";
  import ChevronRight from "@lucide/svelte/icons/chevron-right";
  import CircleStop from "@lucide/svelte/icons/circle-stop";
  import LoaderCircle from "@lucide/svelte/icons/loader-circle";
  import X from "@lucide/svelte/icons/x";
  import {
    commandActivityPresentation,
    fileChangePresentation,
    isCommandActivity,
    isFileChangeActivity,
  } from "$lib/chat/timeline/activity-presentation";
  import type { TimelineActivityRow } from "$lib/chat/timeline/model";
  import { formatNumber } from "$lib/i18n/formatters";
  import { getLocalization } from "$lib/i18n/translator.svelte";

  const localization = getLocalization();
  const { t } = localization;
  let { activity, detail }: { activity: TimelineActivityRow; detail: string | null } = $props();
  const command = $derived(commandActivityPresentation(activity));
  const changes = $derived(fileChangePresentation(activity));
  let expandedChanges = $state<string[]>([]);

  function commandStatus(): string {
    if (command.running) return t("chat.timeline.commandRunning");
    if (activity.status === "interrupted") return t("chat.timeline.commandStopped");
    if (activity.status === "failed" || (command.exitCode !== null && command.exitCode !== 0)) {
      return command.exitCode === null
        ? t("chat.timeline.commandFailed")
        : t("chat.timeline.commandFailedWithCode", formatNumber(localization.locale, command.exitCode));
    }
    return t("chat.timeline.commandSucceeded");
  }

  function toggleChange(path: string): void {
    expandedChanges = expandedChanges.includes(path)
      ? expandedChanges.filter((entry) => entry !== path)
      : [...expandedChanges, path];
  }
</script>

{#if isCommandActivity(activity)}
  <section class="command-card">
    <header>
      <strong>{t("chat.timeline.shell")}</strong>
      {#if command.cwd}<span title={command.cwd}>{command.cwd}</span>{/if}
    </header>
    <pre class="command-line"><span aria-hidden="true">$ </span>{command.command ?? t("chat.timeline.command")}</pre>
    {#if command.output}
      <pre class="command-output">{command.output}</pre>
    {:else if command.running}
      <p class="command-empty">{t("chat.timeline.commandRunning")}</p>
    {:else}
      <p class="command-empty">{t("chat.timeline.noCommandOutput")}</p>
    {/if}
    <footer class:failed={activity.status === "failed" || (command.exitCode !== null && command.exitCode !== 0)}>
      {#if command.running}
        <LoaderCircle class="animate-spin" size={13} />
      {:else if activity.status === "interrupted"}
        <CircleStop size={13} />
      {:else if activity.status === "failed" || (command.exitCode !== null && command.exitCode !== 0)}
        <X size={13} />
      {:else}
        <Check size={13} />
      {/if}
      <span>{commandStatus()}</span>
      {#if command.durationMs !== null}
        <small>{t("chat.timeline.commandDuration", formatNumber(localization.locale, Math.round(command.durationMs)))}</small>
      {/if}
    </footer>
  </section>
{:else if isFileChangeActivity(activity) && changes.length > 0}
  <div class="file-changes">
    {#each changes as change (change.path)}
      {@const expanded = expandedChanges.includes(change.path)}
      <div class="file-change">
        <button type="button" class="file-change-trigger" data-timeline-disclosure-expanded={expanded} aria-expanded={expanded} onclick={() => toggleChange(change.path)}>
          <span class="file-path">{change.path}</span>
          <small>{change.kind}</small>
          {#if change.additions > 0}<span class="additions">+{formatNumber(localization.locale, change.additions)}</span>{/if}
          {#if change.deletions > 0}<span class="deletions">−{formatNumber(localization.locale, change.deletions)}</span>{/if}
          <ChevronRight class={expanded ? "expanded" : undefined} size={13} />
        </button>
        <div class="file-change-region" class:expanded aria-hidden={!expanded} inert={!expanded}>
          <div>{#if change.diff}<pre class="file-diff">{change.diff}</pre>{/if}</div>
        </div>
      </div>
    {/each}
  </div>
{:else if detail}
  <pre class="generic-detail">{detail}</pre>
{/if}

<style>
  .command-card { overflow: hidden; border: 1px solid var(--border); border-radius: 0.75rem; background: color-mix(in srgb, var(--muted) 42%, var(--background)); font-family: "SF Mono", "SFMono-Regular", Consolas, monospace; font-size: calc(0.75rem * var(--type-scale)); }
  .command-card header { display: flex; min-width: 0; align-items: center; gap: 0.6rem; border-bottom: 1px solid var(--border); padding: 0.45rem 0.7rem; font-family: inherit; color: var(--muted-foreground); }
  .command-card header strong { color: var(--foreground); font-family: inherit; font-weight: 500; }
  .command-card header span { min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .command-line, .command-output, .generic-detail, .file-diff { overflow: auto; margin: 0; white-space: pre-wrap; overflow-wrap: anywhere; font-family: "SF Mono", "SFMono-Regular", Consolas, monospace; }
  .command-line { padding: 0.75rem 0.75rem 0.35rem; color: var(--foreground); }
  .command-line span { color: var(--muted-foreground); user-select: none; }
  .command-output { max-height: 18rem; padding: 0.35rem 0.75rem 0.75rem; color: var(--muted-foreground); }
  .command-empty { margin: 0; padding: 0.35rem 0.75rem 0.75rem; color: var(--muted-foreground); }
  .command-card footer { display: flex; align-items: center; justify-content: flex-end; gap: 0.35rem; border-top: 1px solid var(--border); padding: 0.4rem 0.65rem; color: var(--muted-foreground); }
  .command-card footer.failed { color: var(--destructive); }
  .command-card footer small { margin-left: 0.25rem; color: var(--muted-foreground); }
  .file-changes { display: grid; gap: 0.25rem; }
  .file-change { overflow: hidden; border: 1px solid var(--border); border-radius: 0.55rem; background: color-mix(in srgb, var(--muted) 30%, transparent); }
  .file-change-trigger { display: flex; width: 100%; min-width: 0; cursor: pointer; align-items: center; gap: 0.45rem; padding: 0.45rem 0.6rem; text-align: left; }
  .file-change-trigger:focus-visible { outline: 2px solid var(--ring); outline-offset: -2px; }
  .file-change-trigger :global(svg) { flex: 0 0 auto; transition: transform 180ms ease; }
  .file-change-trigger :global(svg.expanded) { transform: rotate(90deg); }
  .file-change-region { display: grid; grid-template-rows: 0fr; opacity: 0; transition: grid-template-rows 420ms cubic-bezier(0.22, 1, 0.36, 1), opacity 180ms ease; }
  .file-change-region.expanded { grid-template-rows: 1fr; opacity: 1; transition: grid-template-rows 420ms cubic-bezier(0.22, 1, 0.36, 1), opacity 240ms ease 55ms; }
  .file-change-region > div { min-height: 0; overflow: hidden; transform: translateY(-0.25rem); transition: transform 360ms cubic-bezier(0.22, 1, 0.36, 1); }
  .file-change-region.expanded > div { transform: translateY(0); }
  .file-path { min-width: 0; flex: 1; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; font-family: "SF Mono", "SFMono-Regular", Consolas, monospace; }
  .file-changes small { color: var(--muted-foreground); }
  .additions { color: var(--action-confirm); }
  .deletions { color: var(--destructive); }
  .file-diff { max-height: 20rem; border-top: 1px solid var(--border); padding: 0.7rem; color: var(--muted-foreground); }
  .generic-detail { max-height: 18rem; border-left: 1px solid var(--border); padding: 0.2rem 0.75rem; color: var(--muted-foreground); }
  @media (prefers-reduced-motion: reduce) { .file-change-trigger :global(svg), .file-change-region, .file-change-region > div { transition: none; } }
</style>
