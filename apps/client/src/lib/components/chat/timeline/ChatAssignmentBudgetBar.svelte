<script lang="ts">
  import type { ChatAssignmentBudgetInput, ChatAssignmentBudgetRead } from "$lib/chat/contracts";
  import {
    MAX_TOKEN_LIMIT,
    MAX_WORKING_SECONDS_LIMIT,
    MIN_TOKEN_LIMIT,
    MIN_WORKING_SECONDS_LIMIT,
    budgetAttention,
    budgetMeters,
    formatMeterAmount,
    parseTokenLimitInput,
    parseWorkingMinutesInput,
    warningPercent,
    workingMinutesFieldValue,
    type BudgetMeterKind,
  } from "$lib/chat/budgets/budget-model";
  import { chatErrorMessage } from "$lib/chat/error-presentation";
  import { formatNumber } from "$lib/i18n/formatters";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import Notice from "$lib/components/ui/Notice.svelte";

  /**
   * Usage of one assignment against its budget, with the warning at 75 and 90 percent and, once
   * a limit stopped the work, the actions to raise the limit and retry.
   */
  let {
    budget,
    teammateName,
    stoppedAt = null,
    onSaveLimits,
    onRetry,
  }: {
    budget: ChatAssignmentBudgetRead;
    teammateName: string;
    /** The limit that stopped the work, or `null` while the work has not stopped at its budget. */
    stoppedAt?: BudgetMeterKind | null;
    /** Saves new limits; `null` removes the budget. */
    onSaveLimits: (budget: ChatAssignmentBudgetInput | null) => Promise<void>;
    onRetry: () => unknown;
  } = $props();

  const localization = getLocalization();
  const t = localization.t;
  const meters = $derived(budgetMeters(budget));
  const attention = $derived(budgetAttention(meters));
  const percent = $derived(attention ? warningPercent(attention.threshold) : null);
  let editing = $state(false);
  let saving = $state(false);
  let tokenText = $state("");
  let minutesText = $state("");
  let saveError = $state<string | null>(null);
  const tokenInput = $derived(parseTokenLimitInput(tokenText));
  const minutesInput = $derived(parseWorkingMinutesInput(minutesText));
  const canSave = $derived(!saving && tokenInput.kind !== "invalid" && minutesInput.kind !== "invalid");

  function meterLabel(kind: BudgetMeterKind): string {
    return kind === "tokens" ? t("chat.composer.budget.tokens") : t("chat.composer.budget.workingTime");
  }

  function warningMessage(kind: BudgetMeterKind, value: number): string {
    const formatted = formatNumber(localization.locale, value);
    return kind === "tokens"
      ? t("chat.composer.budget.tokenWarning", teammateName, formatted)
      : t("chat.composer.budget.workingTimeWarning", teammateName, formatted);
  }

  function reachedMessage(kind: BudgetMeterKind): string {
    return kind === "tokens"
      ? t("chat.composer.budget.tokenReached", teammateName)
      : t("chat.composer.budget.workingTimeReached", teammateName);
  }

  function beginRaise(): void {
    tokenText = budget.tokenLimit === null ? "" : String(budget.tokenLimit);
    minutesText = workingMinutesFieldValue(budget.workingSecondsLimit);
    saveError = null;
    editing = true;
  }

  async function save(event: SubmitEvent): Promise<void> {
    event.preventDefault();
    if (!canSave) return;
    const tokenLimit = tokenInput.kind === "valid" ? tokenInput.value : null;
    const workingSecondsLimit = minutesInput.kind === "valid" ? minutesInput.value : null;
    saving = true;
    saveError = null;
    try {
      await onSaveLimits(tokenLimit === null && workingSecondsLimit === null
        ? null
        : { tokenLimit, workingSecondsLimit });
      editing = false;
    } catch (cause) {
      saveError = chatErrorMessage(cause, t("chat.composer.budget.saveFailed"));
    } finally {
      saving = false;
    }
  }
</script>

<section class="budget-bar surface-floating" aria-label={t("chat.composer.budget.label")} data-assignment-budget>
  {#if meters.length > 0}
    <ul class="budget-meters">
      {#each meters as meter (meter.kind)}
        <li data-budget-meter={meter.kind} data-threshold={meter.threshold}>
          <span class="budget-meter-label">{meterLabel(meter.kind)}</span>
          <span class="budget-meter-amount">{t(
            "chat.composer.budget.usedOfLimit",
            formatMeterAmount(localization.locale, meter.kind, meter.used),
            formatMeterAmount(localization.locale, meter.kind, meter.limit),
          )}</span>
          <span
            class="budget-meter-track"
            role="meter"
            aria-label={meterLabel(meter.kind)}
            aria-valuemin={0}
            aria-valuemax={meter.limit}
            aria-valuenow={Math.min(meter.used, meter.limit)}
          ><span class="budget-meter-fill" style:width={`${meter.fraction * 100}%`}></span></span>
        </li>
      {/each}
    </ul>
  {/if}

  {#if stoppedAt}
    <Notice role="status" tone="warning" message={reachedMessage(stoppedAt)} retryLabel={t("chat.organization.retry")} onRetry={onRetry} copy={false}>
      {#snippet actions()}
        {#if !editing}
          <button type="button" class="notice-action-text" onclick={beginRaise}>{t("chat.composer.budget.raiseLimit")}</button>
        {/if}
      {/snippet}
    </Notice>
  {:else if attention && percent !== null}
    <Notice role="status" tone="warning" message={warningMessage(attention.kind, percent)} copy={false} />
  {/if}

  {#if editing}
    <form class="budget-form" onsubmit={save}>
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
      {:else if saveError}
        <small class="budget-error" role="alert">{saveError}</small>
      {/if}
      <div class="budget-form-actions">
        <button type="button" class="notice-action-text" onclick={() => { editing = false; }}>{t("chat.composer.budget.cancel")}</button>
        <button type="submit" class="notice-action-text" disabled={!canSave}>{t("chat.composer.budget.saveLimits")}</button>
      </div>
    </form>
  {/if}
</section>

<style>
  .budget-bar {
    display: grid;
    gap: 0.375rem;
    padding: 0.5rem 0.625rem;
  }

  .budget-meters {
    display: grid;
    gap: 0.25rem;
    margin: 0;
    padding: 0;
    list-style: none;
  }

  .budget-meters li {
    display: grid;
    grid-template-columns: minmax(0, 1fr) auto;
    gap: 0.125rem 0.5rem;
    align-items: center;
    font-size: calc(0.7rem * var(--type-scale));
  }

  .budget-meter-label {
    color: var(--muted-foreground);
  }

  .budget-meter-amount {
    font-variant-numeric: tabular-nums;
  }

  .budget-meter-track {
    grid-column: 1 / -1;
    height: 0.25rem;
    overflow: hidden;
    border-radius: 999px;
    background-color: color-mix(in oklab, var(--muted-foreground) 20%, transparent);
  }

  .budget-meter-fill {
    display: block;
    height: 100%;
    background-color: var(--primary);
  }

  [data-threshold="warning75"] .budget-meter-fill,
  [data-threshold="warning90"] .budget-meter-fill,
  [data-threshold="reached"] .budget-meter-fill {
    background-color: var(--status-tentative);
  }

  .budget-form {
    display: grid;
    grid-template-columns: repeat(auto-fit, minmax(9rem, 1fr));
    gap: 0.375rem 0.5rem;
  }

  .budget-form label {
    display: grid;
    gap: 0.125rem;
    font-size: calc(0.7rem * var(--type-scale));
    color: var(--muted-foreground);
  }

  .budget-form input {
    color: var(--foreground);
  }

  .budget-error {
    grid-column: 1 / -1;
    color: var(--destructive);
    font-size: calc(0.68rem * var(--type-scale));
  }

  .budget-form-actions {
    grid-column: 1 / -1;
    display: flex;
    justify-content: flex-end;
    gap: 0.5rem;
  }
</style>
