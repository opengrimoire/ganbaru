<script lang="ts">
  import { onMount, tick } from "svelte";
  import AtSign from "@lucide/svelte/icons/at-sign";
  import X from "@lucide/svelte/icons/x";
  import * as chatApi from "$lib/api/chat";
  import { agentNameProblem, lineDisplayName, MAX_AGENT_NAME_CHARACTERS, type AgentNameProblem } from "$lib/chat/agents/agent-names";
  import { customAgentModelChoices, type CustomAgentModelChoice } from "$lib/chat/agents/agent-roster";
  import { customAgentControls, selectedModelOption, withModelOption } from "$lib/chat/agents/custom-agent";
  import { defaultModelOptions, recommendedProviderModel } from "$lib/chat/composer/model";
  import { formatModelDisplayName } from "$lib/chat/composer/model-company";
  import { compactModelOptionLabel } from "$lib/chat/composer/model-picker-model";
  import type { ModelOptionSelection, ProviderInstanceRead, ProviderModel } from "$lib/chat/contracts";
  import { chatErrorMessage } from "$lib/chat/error-presentation";
  import { SETTINGS_PRIMARY_BUTTON_CLASS, SETTINGS_SECONDARY_BUTTON_CLASS } from "$lib/components/settings/action-buttons";
  import Notice from "$lib/components/ui/Notice.svelte";
  import Select from "$lib/components/ui/Select.svelte";
  import { formatNumber } from "$lib/i18n/formatters";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import { activateModalFocus, activateModalKeyboardLayer, trapModalTabKey } from "$lib/modal-focus";
  import { BUILD_PLATFORM_PROFILE, platformHasCapability } from "$lib/platform";
  import { getChat } from "$lib/stores/chat.svelte";
  import { getMobileBackStack } from "$lib/stores/mobile-back-stack.svelte";
  import { cn } from "$lib/utils";
  import { createBackdropPress } from "$lib/utils/backdrop-press";

  /**
   * Creates an agent a person names, such as `@Reviewer`, that runs one model of `provider`, or the newest model of one
   * of its lines, with the reasoning and speed chosen here. The reasoning starts on medium, else the model's highest.
   */
  let { provider, onClose }: { provider: ProviderInstanceRead; onClose: () => void } = $props();

  const FAST_VALUE = "fast";
  const STANDARD_VALUE = "standard";

  const localization = getLocalization();
  const { t } = localization;
  const chat = getChat();
  const mobileBackStack = getMobileBackStack();
  const androidSystemBackAvailable = platformHasCapability(BUILD_PLATFORM_PROFILE, "system.android-back");
  const backdropPress = createBackdropPress();

  let dialog = $state<HTMLDivElement | null>(null);
  let nameInput = $state<HTMLInputElement | null>(null);
  let name = $state("");
  let nameTouched = $state(false);
  let creating = $state(false);
  let error = $state<string | null>(null);
  let chosenModelId = $state<string | null>(null);
  let chosenOptions = $state<ModelOptionSelection[] | null>(null);

  const familyId = $derived(provider.configuration.familyId);
  const choices = $derived(customAgentModelChoices(
    familyId,
    (provider.modelCatalog?.models ?? []).filter((model) => model.availability !== "deprecated"),
    new Intl.Collator(localization.locale).compare,
  ));
  const recommendedId = $derived(recommendedProviderModel(provider, choices.map((entry) => entry.model))?.id ?? null);
  /** The chosen model, else the first choice that runs the recommended model, so a line wins over its version. */
  const choice = $derived<CustomAgentModelChoice | null>(
    choices.find((entry) => entry.modelId === chosenModelId)
      ?? choices.find((entry) => entry.model.id === recommendedId)
      ?? choices[0]
      ?? null,
  );
  const model = $derived<ProviderModel | null>(choice?.model ?? null);
  const options = $derived(chosenOptions ?? (model ? defaultModelOptions(model.options) : []));
  const controls = $derived(customAgentControls(model?.options ?? []));
  const modelChoices = $derived(choices.map((entry) => (entry.line
    ? { value: entry.modelId, label: t("settings.agents.roster.lineChoice", lineDisplayName(entry.line)) }
    : { value: entry.modelId, label: formatModelDisplayName(entry.model.displayName) })));
  /** Model, reasoning, and speed share one row when the dialog is wide enough; model keeps its own row otherwise. */
  const modelControlCount = $derived(1 + (controls.reasoning ? 1 : 0) + (controls.speed ? 1 : 0));
  const reasoningChoices = $derived((controls.reasoning?.options ?? []).map((option) => ({
    value: option.value,
    label: compactModelOptionLabel(option.label, familyId, t("chat.composer.light"), t("chat.composer.extraHigh")),
  })));
  const speedChoices = $derived(controls.speed?.kind === "choice"
    ? controls.speed.options.map((option) => ({ value: option.value, label: option.label }))
    : [
        { value: STANDARD_VALUE, label: t("settings.agents.roster.speedStandard") },
        { value: FAST_VALUE, label: t("settings.agents.roster.speedFast") },
      ]);
  const nameProblem = $derived(agentNameProblem(
    name,
    [
      ...chat.agentIdentities.map((agent) => agent.participant.displayName),
      ...(chat.settings?.providerFamilies ?? []).flatMap((family) => [family.familyId, family.displayName]),
    ],
  ));
  const visibleProblem = $derived(nameTouched || name.trim() ? nameProblem : null);

  /** The message under the name field; a missing name shows only the invalid field outline. */
  function problemMessage(problem: Exclude<AgentNameProblem, "empty">): string {
    if (problem === "tooLong") {
      return t("settings.agents.roster.nameProblems.tooLong", formatNumber(localization.locale, MAX_AGENT_NAME_CHARACTERS));
    }
    return t(`settings.agents.roster.nameProblems.${problem}`);
  }

  function choiceValue(key: string | undefined): string {
    if (!key) return "";
    const value = selectedModelOption(options, key);
    return value?.kind === "choice" ? value.value : "";
  }

  function speedValue(): string {
    const speed = controls.speed;
    if (!speed) return "";
    if (speed.kind === "choice") return choiceValue(speed.key);
    const value = selectedModelOption(options, speed.key);
    return value?.kind === "boolean" && value.value ? FAST_VALUE : STANDARD_VALUE;
  }

  function chooseModel(modelId: string): void {
    chosenModelId = modelId;
    chosenOptions = null;
  }

  function chooseReasoning(value: string): void {
    if (!controls.reasoning) return;
    chosenOptions = withModelOption(options, controls.reasoning.key, { kind: "choice", value });
  }

  function chooseSpeed(value: string): void {
    const speed = controls.speed;
    if (!speed) return;
    chosenOptions = withModelOption(
      options,
      speed.key,
      speed.kind === "choice" ? { kind: "choice", value } : { kind: "boolean", value: value === FAST_VALUE },
    );
  }

  async function create(event: SubmitEvent): Promise<void> {
    event.preventDefault();
    nameTouched = true;
    if (nameProblem || creating || !choice) return;
    creating = true;
    error = null;
    try {
      await chatApi.createChatAgent({
        displayName: name.trim(),
        providerFamilyId: familyId,
        policy: {
          providerManagedModel: false,
          modelId: choice.modelId,
          modelOptions: options.map((option) => ({ ...option })),
          providerOptions: { schemaVersion: 1, value: {} },
          interactionMode: "build",
        },
      });
      await chat.refreshAgents();
      onClose();
    } catch (cause: unknown) {
      error = chatErrorMessage(cause, t("settings.agents.roster.createFailed"));
    } finally {
      creating = false;
    }
  }

  function handleKeydown(event: KeyboardEvent): void {
    if (event.key === "Tab" && dialog) {
      trapModalTabKey(dialog, event);
      return;
    }
    if (event.key !== "Escape" || dialog?.querySelector("[data-app-floating-surface]")) return;
    event.preventDefault();
    onClose();
  }

  onMount(() => {
    const deactivateMobileBack = androidSystemBackAvailable
      ? mobileBackStack.activate({ handle: () => onClose() })
      : () => undefined;
    const deactivateKeyboard = activateModalKeyboardLayer(handleKeydown);
    let deactivateFocus = (): void => undefined;
    void tick().then(() => {
      if (dialog) deactivateFocus = activateModalFocus(dialog, nameInput);
    });
    return () => {
      deactivateMobileBack();
      deactivateKeyboard();
      deactivateFocus();
    };
  });
</script>

<!-- svelte-ignore a11y_click_events_have_key_events -->
<!-- svelte-ignore a11y_no_static_element_interactions -->
<div
  class="fixed z-90 flex items-center justify-center"
  style="left: var(--visual-viewport-offset-left); top: var(--visual-viewport-offset-top); width: var(--visual-viewport-width); height: var(--visual-viewport-height); padding: calc(var(--safe-area-top) + 1rem) calc(var(--safe-area-right) + 1rem) calc(var(--safe-area-bottom) + 1rem) calc(var(--safe-area-left) + 1rem);"
  onpointerdown={backdropPress.start}
  onclick={(event) => { event.stopPropagation(); if (backdropPress.isClick(event)) onClose(); }}
>
  <div class="surface-backdrop absolute inset-0"></div>
  <div
    bind:this={dialog}
    class="custom-agent-dialog surface-dialog relative z-10 flex max-h-full w-full max-w-md flex-col outline-none"
    style="--foreground: var(--card-foreground);"
    role="dialog"
    aria-modal="true"
    aria-labelledby="custom-agent-dialog-title"
    tabindex="-1"
    data-floating-root
    onclick={(event) => event.stopPropagation()}
  >
    <form class="contents" onsubmit={(event) => void create(event)}>
      <header class="flex items-center justify-between gap-3 px-5 pt-4 pb-1">
        <h2 id="custom-agent-dialog-title" class="truncate text-[1rem] font-semibold">{t("settings.agents.roster.dialogTitle")}</h2>
        <button type="button" class="dialog-icon-button" aria-label={t("common.close")} onclick={onClose}><X size={16} /></button>
      </header>

      <div class="@container flex min-h-0 flex-col gap-4 overflow-y-auto px-5 py-3 text-panel">
        <label class="grid gap-1.5">
          <span class="field-label">{t("settings.agents.roster.nameLabel")}</span>
          <span class="field flex min-w-0 items-center gap-1 pl-2.5">
            <AtSign size={14} strokeWidth={1.9} class="shrink-0 text-muted-foreground" aria-hidden="true" />
            <input
              bind:this={nameInput}
              class="field-bare min-w-0 flex-1"
              bind:value={name}
              placeholder={t("settings.agents.roster.namePlaceholder")}
              maxlength={MAX_AGENT_NAME_CHARACTERS}
              autocomplete="off"
              spellcheck="false"
              disabled={creating}
              aria-invalid={visibleProblem !== null}
              onblur={() => { nameTouched = true; }}
            />
          </span>
          {#if visibleProblem && visibleProblem !== "empty"}<span class="text-panel-detail text-destructive">{problemMessage(visibleProblem)}</span>{/if}
        </label>

        <div class={cn("grid gap-4 @xs:grid-cols-2 @xs:gap-3", modelControlCount === 3 && "@sm:grid-cols-[minmax(0,1.6fr)_minmax(0,1fr)_minmax(0,1fr)]")}>
          <div class={cn("grid min-w-0 gap-1.5", modelControlCount !== 2 && "@xs:col-span-2", modelControlCount === 3 && "@sm:col-span-1")}>
            <span class="field-label">{t("settings.agents.roster.modelLabel")}</span>
            <Select
              inline
              class="w-full"
              value={choice?.modelId ?? ""}
              options={modelChoices}
              ariaLabel={t("settings.agents.roster.modelLabel")}
              disabled={creating || modelChoices.length === 0}
              onChange={chooseModel}
            />
          </div>

          {#if controls.reasoning}
            <div class="grid min-w-0 gap-1.5">
              <span class="field-label">{t("settings.agents.roster.reasoningLabel")}</span>
              <Select
                inline
                class="w-full"
                value={choiceValue(controls.reasoning.key)}
                options={reasoningChoices}
                ariaLabel={t("settings.agents.roster.reasoningLabel")}
                disabled={creating}
                onChange={chooseReasoning}
              />
            </div>
          {/if}

          {#if controls.speed}
            <div class="grid min-w-0 gap-1.5">
              <span class="field-label">{t("settings.agents.roster.speedLabel")}</span>
              <Select
                inline
                class="w-full"
                value={speedValue()}
                options={speedChoices}
                ariaLabel={t("settings.agents.roster.speedLabel")}
                disabled={creating}
                onChange={chooseSpeed}
              />
            </div>
          {/if}
        </div>

        {#if error}<Notice class="text-panel-detail" message={error} copy={false} />{/if}
      </div>

      <footer class="flex justify-end gap-2 px-5 pt-2 pb-4">
        <button type="button" class={SETTINGS_SECONDARY_BUTTON_CLASS} disabled={creating} onclick={onClose}>{t("settings.agents.roster.cancel")}</button>
        <button
          type="submit"
          class={SETTINGS_PRIMARY_BUTTON_CLASS}
          disabled={creating || !choice || (nameTouched && nameProblem !== null)}
        >{t("settings.agents.roster.create")}</button>
      </footer>
    </form>
  </div>
</div>

<style>
  .custom-agent-dialog { max-height: min(36rem, 100%); }
  .field-label { color: var(--muted-foreground); font-size: var(--panel-detail-font-size); font-weight: 500; line-height: 1.4; }
  .dialog-icon-button { display: grid; width: 1.75rem; height: 1.75rem; flex: 0 0 auto; place-items: center; border-radius: 0.375rem; color: var(--muted-foreground); }
  .dialog-icon-button:hover:not(:disabled) { background: var(--accent); color: var(--foreground); }
  @media (pointer: coarse) { .dialog-icon-button { width: 2.5rem; height: 2.5rem; } }
</style>
