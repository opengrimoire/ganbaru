<script lang="ts">
  import type { ProviderPreferenceEdit, ProviderPreferenceRead, ProviderPreferencesRead } from "$lib/chat/contracts";
  import {
    providerPreferenceInputValue,
    providerPreferenceName,
    providerPreferenceOptionName,
    type ProviderPreferenceDrafts,
  } from "$lib/chat/provider-files";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import ResetButton from "$lib/components/ui/ResetButton.svelte";
  import Select from "$lib/components/ui/Select.svelte";
  import Switch from "$lib/components/ui/Switch.svelte";

  type PreferenceValue = ProviderPreferenceEdit["value"];

  /**
   * Typed controls for the catalog preferences of one provider configuration file. Each change
   * becomes a draft through `onChange`, where null removes the entry, and the page's footer saves it.
   */
  let {
    preferences,
    allowlisted,
    disabled,
    drafts,
    onChange,
  }: {
    preferences: ProviderPreferencesRead;
    /** Whether agent runs skip values that fail their check, which decides the warning shown. */
    allowlisted: boolean;
    disabled: boolean;
    drafts: ProviderPreferenceDrafts;
    onChange: (preferenceId: string, value: PreferenceValue) => void;
  } = $props();

  const { t } = getLocalization();
  const locked = $derived(disabled || !preferences.editable);
  const booleanOptions = $derived([
    { value: "default", label: t("settings.agents.files.preferenceDefault") },
    { value: "on", label: t("settings.agents.files.preferenceOn") },
    { value: "off", label: t("settings.agents.files.preferenceOff") },
  ]);

  function label(entry: ProviderPreferenceRead): string {
    const name = providerPreferenceName(entry.id);
    return name ? t(`settings.agents.files.preferences.${name}`) : entry.id;
  }

  function drafted(entry: ProviderPreferenceRead): boolean {
    return Object.hasOwn(drafts, entry.id);
  }

  /** The value the control shows: the unsaved draft, or the saved value. */
  function current(entry: ProviderPreferenceRead): PreferenceValue {
    return drafted(entry) ? drafts[entry.id] ?? null : entry.value;
  }

  function booleanValue(entry: ProviderPreferenceRead): string {
    const value = current(entry);
    if (value === true) return "on";
    if (value === false) return "off";
    return "default";
  }

  const DEFAULT_CHOICE = "";

  function choiceOptions(entry: ProviderPreferenceRead): { value: string; label: string }[] {
    return [
      { value: DEFAULT_CHOICE, label: t("settings.agents.files.preferenceDefault") },
      ...entry.options.map((option) => {
        const name = providerPreferenceOptionName(option);
        return { value: option, label: name ? t(`settings.agents.files.preferenceOptions.${name}`) : option };
      }),
    ];
  }

  function changeChoice(entry: ProviderPreferenceRead, choice: string): void {
    onChange(entry.id, choice === DEFAULT_CHOICE ? null : choice);
  }

  function changeBoolean(entry: ProviderPreferenceRead, choice: string): void {
    onChange(entry.id, choice === "on" ? true : choice === "off" ? false : null);
  }

  function commitText(entry: ProviderPreferenceRead, input: string): void {
    const value = providerPreferenceInputValue(entry, input);
    if (value === current(entry)) return;
    onChange(entry.id, value);
  }

  /** Whether the entry is edited in a text box, which its label then names. */
  function usesTextInput(entry: ProviderPreferenceRead): boolean {
    return entry.valueType !== "switch" && entry.valueType !== "boolean" && entry.valueType !== "choice";
  }

  function textValue(entry: ProviderPreferenceRead): string {
    const value = current(entry);
    return typeof value === "string" ? value : "";
  }

  function blurOnEnter(event: KeyboardEvent & { currentTarget: HTMLInputElement }): void {
    if (event.key === "Enter") event.currentTarget.blur();
  }
</script>

{#snippet details(entry: ProviderPreferenceRead, controlId: string | null)}
  <div class="min-w-0 flex-1">
    {#if controlId}
      <label for={controlId} class="text-[0.866667rem] text-foreground">{label(entry)}</label>
    {:else}
      <div class="text-[0.866667rem] text-foreground">{label(entry)}</div>
    {/if}
    <div class="mt-0.5 wrap-break-word text-[0.8rem] leading-5 text-muted-foreground">{entry.variable ? `env.${entry.id}` : entry.id}</div>
    {#if entry.ignored && !drafted(entry)}
      <p class="mt-0.5 text-[0.8rem] leading-5 text-status-tentative">{t(allowlisted ? "settings.agents.files.preferenceIgnored" : "settings.agents.files.preferenceInvalid")}</p>
    {/if}
  </div>
{/snippet}

{#snippet resetButton(entry: ProviderPreferenceRead)}
  <ResetButton
    canReset={!locked && current(entry) !== null}
    label={t("settings.agents.files.preferenceReset", label(entry))}
    onReset={() => onChange(entry.id, null)}
  />
{/snippet}

<div class="flex flex-col gap-3">
  {#each preferences.entries as entry (entry.id)}
    {@const controlId = `agent-preference-${entry.id.replaceAll(".", "-")}`}
    {#if entry.multiline}
      <div class="flex flex-col gap-2 px-1 py-1">
        <div class="flex items-start justify-between gap-4">
          {@render details(entry, controlId)}
          {@render resetButton(entry)}
        </div>
        <textarea
          id={controlId}
          class="field min-h-16 w-full resize-y px-2.5 py-1.5 font-mono text-[0.766667rem] text-foreground"
          value={textValue(entry)}
          disabled={locked}
          spellcheck="false"
          oninput={(event) => commitText(entry, event.currentTarget.value)}
        ></textarea>
      </div>
    {:else}
      <div class="flex items-center justify-between gap-4 px-1 py-1 max-[480px]:flex-col max-[480px]:items-stretch max-[480px]:gap-2">
        {@render details(entry, usesTextInput(entry) ? controlId : null)}
        <div class="flex items-center justify-end gap-1.5 max-[480px]:justify-between">
          {#if entry.valueType === "switch"}
            <Switch checked={current(entry) === true} disabled={locked} ariaLabel={label(entry)} onChange={(checked) => onChange(entry.id, checked)} />
          {:else if entry.valueType === "boolean"}
            <Select inline value={booleanValue(entry)} options={booleanOptions} disabled={locked} ariaLabel={label(entry)} onChange={(choice) => changeBoolean(entry, choice)} />
          {:else if entry.valueType === "choice"}
            <Select inline value={textValue(entry)} options={choiceOptions(entry)} disabled={locked} ariaLabel={label(entry)} onChange={(choice) => changeChoice(entry, choice)} />
          {:else}
            <input
              id={controlId}
              class="field w-44 min-w-0 px-2.5 text-left text-[0.8rem] font-medium text-foreground max-[480px]:w-full"
              value={textValue(entry)}
              inputmode={entry.valueType === "text" ? "text" : "numeric"}
              placeholder={t("settings.agents.files.preferenceDefault")}
              disabled={locked}
              spellcheck="false"
              autocapitalize="off"
              onkeydown={blurOnEnter}
              oninput={(event) => commitText(entry, event.currentTarget.value)}
            />
          {/if}
          {@render resetButton(entry)}
        </div>
      </div>
    {/if}
  {/each}
</div>
