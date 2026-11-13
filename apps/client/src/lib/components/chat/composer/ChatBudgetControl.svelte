<!--
  @component
  The budget the new work of a post starts with: the teammate default, or token and working time
  limits chosen for this post. It is disabled with a reason where a post cannot carry a budget.
-->
<script lang="ts">
  import Gauge from "@lucide/svelte/icons/gauge";
  import type { ChatAssignmentBudgetInput } from "$lib/chat/contracts";
  import {
    MAX_TOKEN_LIMIT,
    MAX_WORKING_SECONDS_LIMIT,
    MIN_TOKEN_LIMIT,
    MIN_WORKING_SECONDS_LIMIT,
    formatTokenCount,
    formatWorkingDuration,
    parseTokenLimitInput,
    parseWorkingMinutesInput,
    workingMinutesFieldValue,
  } from "$lib/chat/budgets/budget-model";
  import { formatList, formatNumber } from "$lib/i18n/formatters";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import { isAppFloatingSurfaceTarget } from "$lib/utils";
  import { keepInViewport } from "$lib/utils/keep-in-viewport";

  let {
    budget,
    teammateDefaultTokens,
    disabledReason,
    onChange,
  }: {
    /** The limits chosen for this post, or `null` for the teammate default. */
    budget: ChatAssignmentBudgetInput | null;
    /** The teammate's default token limit, or `null` when its work has no limit by default. */
    teammateDefaultTokens: number | null;
    /** Why no budget can be chosen for this post, or `null` when one can. */
    disabledReason: string | null;
    onChange: (budget: ChatAssignmentBudgetInput | null) => void;
  } = $props();

  const localization = getLocalization();
  const t = localization.t;
  let open = $state(false);
  let root = $state<HTMLSpanElement | undefined>();
  let tokenText = $state("");
  let minutesText = $state("");
  const tokenInput = $derived(parseTokenLimitInput(tokenText));
  const minutesInput = $derived(parseWorkingMinutesInput(minutesText));
  const canSave = $derived(tokenInput.kind !== "invalid" && minutesInput.kind !== "invalid");
  const defaultLabel = $derived(teammateDefaultTokens === null
    ? t("chat.composer.budget.noTeammateDefault")
    : t("chat.composer.budget.teammateDefault", formatTokenCount(localization.locale, teammateDefaultTokens)));
  const summary = $derived(budget && !disabledReason ? budgetSummary(budget) : defaultLabel);
  const label = $derived(disabledReason
    ? `${t("chat.composer.budget.label")}: ${disabledReason}`
    : `${t("chat.composer.budget.label")}: ${summary}`);

  function budgetSummary(value: ChatAssignmentBudgetInput): string {
    const parts: string[] = [];
    if (value.tokenLimit !== null) {
      parts.push(t("chat.composer.budget.tokenSummary", formatTokenCount(localization.locale, value.tokenLimit)));
    }
    if (value.workingSecondsLimit !== null) {
      parts.push(formatWorkingDuration(localization.locale, value.workingSecondsLimit));
    }
    return t("chat.composer.budget.custom", formatList(localization.locale, parts, { type: "unit", style: "short" }));
  }

  function toggle(): void {
    if (open) {
      open = false;
      return;
    }
    tokenText = budget && budget.tokenLimit !== null ? String(budget.tokenLimit) : "";
    minutesText = workingMinutesFieldValue(budget?.workingSecondsLimit ?? null);
    open = true;
  }

  function save(event: SubmitEvent): void {
    event.preventDefault();
    if (!canSave) return;
    const tokenLimit = tokenInput.kind === "valid" ? tokenInput.value : null;
    const workingSecondsLimit = minutesInput.kind === "valid" ? minutesInput.value : null;
    onChange(tokenLimit === null && workingSecondsLimit === null ? null : { tokenLimit, workingSecondsLimit });
    open = false;
  }

  function useDefault(): void {
    onChange(null);
    open = false;
  }

  $effect(() => {
    if (disabledReason) open = false;
  });

  $effect(() => {
    if (!open) return;
    const closeFromOutside = (event: PointerEvent) => {
      const target = event.target;
      if (!(target instanceof Node) || root?.contains(target) || isAppFloatingSurfaceTarget(target)) return;
      open = false;
    };
    const closeOnEscape = (event: KeyboardEvent) => {
      if (event.key !== "Escape") return;
      event.stopPropagation();
      open = false;
    };
    window.addEventListener("pointerdown", closeFromOutside, true);
    window.addEventListener("keydown", closeOnEscape, true);
    return () => {
      window.removeEventListener("pointerdown", closeFromOutside, true);
      window.removeEventListener("keydown", closeOnEscape, true);
    };
  });
</script>

<span bind:this={root} class="budget-control" data-chat-budget-control>
  <button
    type="button"
    class="budget-trigger"
    class:active={open || (budget !== null && !disabledReason)}
    aria-label={label}
    aria-haspopup="dialog"
    aria-expanded={open}
    title={disabledReason ?? summary}
    disabled={disabledReason !== null}
    onclick={toggle}
  ><Gauge size={14} aria-hidden="true" /><span class="budget-trigger-text">{summary}</span></button>
  {#if open}
    <div use:keepInViewport class="budget-popover surface-floating surface-floating-body" role="dialog" aria-label={t("chat.composer.budget.label")}>
      <form class="budget-form" onsubmit={save}>
        <p class="budget-default">{defaultLabel}</p>
        <label>
          <span>{t("chat.composer.budget.tokenLimit")}</span>
          <input class="field" bind:value={tokenText} inputmode="numeric" autocomplete="off" placeholder={t("chat.composer.budget.noLimit")} aria-invalid={tokenInput.kind === "invalid" ? "true" : undefined} />
        </label>
        <label>
          <span>{t("chat.composer.budget.workingMinutes")}</span>
          <input class="field" bind:value={minutesText} inputmode="numeric" autocomplete="off" placeholder={t("chat.composer.budget.noLimit")} aria-invalid={minutesInput.kind === "invalid" ? "true" : undefined} />
        </label>
        {#if tokenInput.kind === "invalid"}
          <small class="budget-error" role="alert">{t("chat.composer.budget.invalidTokens", formatNumber(localization.locale, MIN_TOKEN_LIMIT), formatNumber(localization.locale, MAX_TOKEN_LIMIT))}</small>
        {:else if minutesInput.kind === "invalid"}
          <small class="budget-error" role="alert">{t("chat.composer.budget.invalidMinutes", formatNumber(localization.locale, MIN_WORKING_SECONDS_LIMIT / 60), formatNumber(localization.locale, MAX_WORKING_SECONDS_LIMIT / 60))}</small>
        {/if}
        <div class="budget-actions">
          <button type="button" class="notice-action-text" onclick={useDefault}>{t("chat.composer.budget.useTeammateDefault")}</button>
          <button type="submit" class="notice-action-text" disabled={!canSave}>{t("chat.composer.budget.saveLimits")}</button>
        </div>
      </form>
    </div>
  {/if}
</span>

<style>
  .budget-control { position: relative; display: inline-flex; min-width: 0; }
  .budget-trigger { display: inline-flex; min-width: 0; max-width: 11rem; align-items: center; gap: 0.3rem; border-radius: 0.5rem; padding: 0.2rem 0.4rem; color: var(--muted-foreground); font-size: var(--panel-detail-font-size); }
  .budget-trigger:hover:not(:disabled), .budget-trigger.active { background: var(--accent); color: var(--foreground); }
  .budget-trigger:disabled { opacity: 0.45; }
  .budget-trigger-text { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .budget-popover { position: absolute; z-index: 70; right: 0; bottom: calc(100% + 0.35rem); width: min(17rem, calc(100vw - 1rem)); }
  .budget-form { display: grid; gap: 0.4rem; }
  .budget-default { margin: 0; color: var(--muted-foreground); font-size: var(--panel-detail-font-size); }
  .budget-popover label { display: grid; gap: 0.125rem; color: var(--muted-foreground); font-size: var(--panel-detail-font-size); }
  .budget-popover input { color: var(--foreground); }
  .budget-error { color: var(--destructive); font-size: var(--panel-detail-font-size); }
  .budget-actions { display: flex; flex-wrap: wrap; justify-content: flex-end; gap: 0.5rem; }
  @media (pointer: coarse) { .budget-trigger { min-height: 2.75rem; } }
</style>
