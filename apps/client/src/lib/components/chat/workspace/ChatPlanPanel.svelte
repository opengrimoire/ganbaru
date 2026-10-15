<script lang="ts">
  import Check from "@lucide/svelte/icons/check";
  import Circle from "@lucide/svelte/icons/circle";
  import Copy from "@lucide/svelte/icons/copy";
  import LoaderCircle from "@lucide/svelte/icons/loader-circle";
  import Play from "@lucide/svelte/icons/play";
  import X from "@lucide/svelte/icons/x";
  import { chatErrorMessage } from "$lib/chat/error-presentation";
  import { projectTimelineReadModel, type TimelinePlanRow } from "$lib/chat/timeline/model";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import { useChat } from "$lib/stores/chat.svelte";
  import ChatMarkdown from "$lib/components/chat/timeline/ChatMarkdown.svelte";
  import Notice from "$lib/components/ui/Notice.svelte";

  const { t } = getLocalization();
  const chat = useChat();
  let dismissedByThread = $state<Record<string, string[]>>({});
  let error = $state<string | null>(null);
  const pageTurns = $derived(chat.timelinePages.flatMap((page) => page.turns));
  const projection = $derived(projectTimelineReadModel(chat.timelineItems, pageTurns));
  const plans = $derived(projection.rows
    .filter((row): row is TimelinePlanRow => row.kind === "plan")
    .filter((row) => !(dismissedByThread[chat.selectedThreadId ?? ""] ?? []).includes(row.id))
    .toReversed());
  const latest = $derived(plans[0] ?? null);

  function implementationExists(plan: TimelinePlanRow): boolean {
    if (!plan.turnId) return false;
    const originIndex = projection.turns.findIndex((turn) => turn.id === plan.turnId);
    return projection.turns.slice(originIndex + 1).some((turn) => turn.modes.interactionMode === "build");
  }

  function prepareTurn(plan: TimelinePlanRow, mode: "plan" | "build"): void {
    const action = mode === "plan" ? t("chat.inspector.continuePlanningPrompt") : t("chat.inspector.implementPlanPrompt");
    const reference = plan.turnId ? `${action} (${t("chat.inspector.originTurn")}: ${plan.turnId})` : action;
    const existing = chat.composer.text.trim();
    chat.setComposerText(existing ? `${existing}\n\n${reference}` : reference);
    chat.setComposerModes(chat.composer.safetyMode, mode);
    queueMicrotask(() => document.querySelector<HTMLElement>("[data-chat-composer]")?.focus());
  }

  function dismiss(planId: string): void {
    const threadId = chat.selectedThreadId;
    if (!threadId) return;
    dismissedByThread = {
      ...dismissedByThread,
      [threadId]: [...(dismissedByThread[threadId] ?? []), planId],
    };
  }

  function statusIcon(status: string): typeof Circle {
    if (status === "completed") return Check;
    if (status === "active") return LoaderCircle;
    return Circle;
  }

  function reportError(reason: unknown): void {
    error = chatErrorMessage(reason);
  }
</script>

<div class="h-full min-h-0 overflow-auto p-3">
  {#if error}<Notice class="mb-2 text-xs" message={error} />{/if}
  {#if latest}
    <article class="space-y-3" aria-label={t("chat.inspector.plan")}>
      <div class="flex flex-wrap items-center gap-2 text-[0.666667rem] text-muted-foreground">
        <span>{latest.planKind === "structured" ? t("chat.inspector.structuredPlan") : t("chat.inspector.proposedPlan")}</span>
        {#if latest.turnId}<span>{t("chat.inspector.originTurn")}: {latest.turnId.slice(0, 8)}</span>{/if}
        <span>{implementationExists(latest) ? t("chat.inspector.implementationExists") : t("chat.inspector.notImplemented")}</span>
      </div>
      <ChatMarkdown markdown={latest.markdown} onError={reportError} />
      {#if latest.steps.length > 0}
        <ol class="space-y-1">
          {#each latest.steps as step (step.id)}
            {@const Icon = statusIcon(step.status)}
            <li class="flex items-start gap-2 rounded border border-border p-2 text-xs">
              <Icon size={13} class={step.status === "active" ? "animate-spin" : ""} />
              <span class="min-w-0 flex-1">{step.text}</span>
              <span class="text-[0.583333rem] text-muted-foreground">{t(`chat.timeline.activityStatus.${step.status}`)}</span>
            </li>
          {/each}
        </ol>
      {/if}
      <div class="flex flex-wrap gap-1">
        <button type="button" class="chat-secondary-button" onclick={() => navigator.clipboard.writeText(latest.markdown)}><Copy size={12} />{t("chat.timeline.copy")}</button>
        <button type="button" class="chat-secondary-button" onclick={() => prepareTurn(latest, "plan")}><Circle size={12} />{t("chat.timeline.continuePlanning")}</button>
        <button type="button" class="chat-secondary-button" onclick={() => prepareTurn(latest, "build")}><Play size={12} />{t("chat.timeline.implementPlan")}</button>
        <button type="button" class="chat-secondary-button" onclick={() => dismiss(latest.id)}><X size={12} />{t("chat.timeline.dismiss")}</button>
      </div>
    </article>
  {:else}
    <p class="m-auto p-4 text-center text-xs text-muted-foreground">{t("chat.inspector.noPlan")}</p>
  {/if}
</div>
