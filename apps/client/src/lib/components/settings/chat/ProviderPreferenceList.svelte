<script lang="ts">
  import RotateCcw from "@lucide/svelte/icons/rotate-ccw";
  import type { ProviderPreferenceRead, ProviderPreferencesRead } from "$lib/chat/contracts";
  import { providerPreferenceInputValue, providerPreferenceName } from "$lib/chat/provider-files";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import Select from "$lib/components/ui/Select.svelte";
  import Switch from "$lib/components/ui/Switch.svelte";

  /**
   * Typed controls for the allowlisted teammate preferences of one provider configuration file.
   * Each change saves the single entry through `onSave`; a null value removes it.
   */
  let {
    preferences,
    disabled,
    onSave,
  }: {
    preferences: ProviderPreferencesRead;
    disabled: boolean;
    onSave: (preferenceId: string, value: boolean | string | null) => Promise<void>;
  } = $props();

  const { t } = getLocalization();
  const locked = $derived(disabled || !preferences.editable);
  const booleanOptions = $derived([
    { value: "default", label: t("settings.chat.permissions.preferenceDefault") },
    { value: "on", label: t("settings.chat.permissions.preferenceOn") },
    { value: "off", label: t("settings.chat.permissions.preferenceOff") },
  ]);

  function label(entry: ProviderPreferenceRead): string {
    const name = providerPreferenceName(entry.id);
    return name ? t(`settings.chat.permissions.preferences.${name}.label`) : entry.id;
  }

  function description(entry: ProviderPreferenceRead): string | null {
    const name = providerPreferenceName(entry.id);
    return name ? t(`settings.chat.permissions.preferences.${name}.description`) : null;
  }

  function booleanValue(entry: ProviderPreferenceRead): string {
    if (entry.value === true) return "on";
    if (entry.value === false) return "off";
    return "default";
  }

  function saveBoolean(entry: ProviderPreferenceRead, choice: string): void {
    void onSave(entry.id, choice === "on" ? true : choice === "off" ? false : null);
  }

  function commitText(entry: ProviderPreferenceRead, input: string): void {
    const value = providerPreferenceInputValue(entry, input);
    if (value === entry.value) return;
    void onSave(entry.id, value);
  }

  function textValue(entry: ProviderPreferenceRead): string {
    return typeof entry.value === "string" ? entry.value : "";
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
    {#if description(entry)}
      <div class="mt-0.5 text-[0.8rem] text-muted-foreground">{description(entry)}</div>
    {/if}
    <code class="mt-0.5 block break-all font-mono text-[0.666667rem] text-muted-foreground">{entry.variable ? `env.${entry.id}` : entry.id}</code>
    {#if entry.ignored}
      <p class="mt-0.5 text-[0.733333rem] text-status-tentative">{t("settings.chat.permissions.preferenceIgnored")}</p>
    {/if}
  </div>
{/snippet}

{#snippet resetButton(entry: ProviderPreferenceRead)}
  <button
    type="button"
    class="chat-settings-button"
    disabled={locked || entry.value === null}
    aria-label={t("settings.chat.permissions.preferenceReset", label(entry))}
    title={t("settings.chat.permissions.preferenceReset", label(entry))}
    onclick={() => void onSave(entry.id, null)}
  >
    <RotateCcw size={12} />
  </button>
{/snippet}

<div class="flex flex-col gap-3">
  {#each preferences.entries as entry (entry.id)}
    {@const controlId = `teammate-preference-${entry.id.replaceAll(".", "-")}`}
    {#if entry.valueType === "switch"}
      <div class="flex items-center justify-between gap-4 px-1 py-1 max-[480px]:items-start">
        {@render details(entry, null)}
        <Switch checked={entry.value === true} disabled={locked} ariaLabel={label(entry)} onChange={(checked) => void onSave(entry.id, checked)} />
      </div>
    {:else if entry.valueType === "boolean"}
      <div class="flex items-center justify-between gap-4 px-1 py-1 max-[480px]:flex-col max-[480px]:items-stretch max-[480px]:gap-2">
        {@render details(entry, null)}
        <Select inline value={booleanValue(entry)} options={booleanOptions} disabled={locked} ariaLabel={label(entry)} onChange={(choice) => saveBoolean(entry, choice)} />
      </div>
    {:else if entry.multiline}
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
          onchange={(event) => commitText(entry, event.currentTarget.value)}
        ></textarea>
      </div>
    {:else}
      <div class="flex items-center justify-between gap-4 px-1 py-1 max-[480px]:flex-col max-[480px]:items-stretch max-[480px]:gap-2">
        {@render details(entry, controlId)}
        <div class="flex items-center justify-end gap-1.5">
          <input
            id={controlId}
            class="field w-44 min-w-0 px-2.5 text-left text-[0.8rem] font-medium text-foreground max-[480px]:w-full"
            value={textValue(entry)}
            inputmode={entry.valueType === "text" ? "text" : "numeric"}
            placeholder={t("settings.chat.permissions.preferenceDefault")}
            disabled={locked}
            spellcheck="false"
            autocapitalize="off"
            onkeydown={blurOnEnter}
            onchange={(event) => commitText(entry, event.currentTarget.value)}
          />
          {@render resetButton(entry)}
        </div>
      </div>
    {/if}
  {/each}
</div>
