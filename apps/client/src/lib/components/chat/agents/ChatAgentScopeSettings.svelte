<script lang="ts" module>
  import type { ChatAgentAccess } from "$lib/chat/contracts";

  /** What a scope saves: its own access, null to inherit, and its instructions. */
  export interface ChatAgentScopeValue {
    access: ChatAgentAccess | null;
    instructions: string;
  }
</script>

<script lang="ts">
  import { untrack } from "svelte";
  import type { ChatEffectiveChannelAccessRead } from "$lib/chat/contracts";
  import {
    CHAT_AGENT_INSTRUCTIONS_LIMIT,
    chatAccessOptions,
    chatScopeSourceLine,
    type ChatInheritingAgentScope,
  } from "$lib/chat/agents/channel-access";
  import { chatErrorMessage } from "$lib/chat/error-presentation";
  import { formatNumber } from "$lib/i18n/formatters";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import Notice from "$lib/components/ui/Notice.svelte";
  import ResetButton from "$lib/components/ui/ResetButton.svelte";
  import Select from "$lib/components/ui/Select.svelte";

  /**
   * Access and instructions of one agent settings scope (app, project, or channel). Access saves as soon as it is
   * chosen; instructions save with their own button, like channel notes. The parent saves with the scope's
   * revision and rereads the scope afterward, also when the save fails.
   */
  let {
    access,
    effective = null,
    instructions,
    ownAvailable = true,
    markDefault = false,
    scope = "channel",
    idPrefix,
    onSave,
  }: {
    /** The scope's own access; null inherits `effective`. The app scope always has its own. */
    access: ChatAgentAccess | null;
    /** What the scope resolves to with its source, or null at the app scope, which inherits nothing. */
    effective?: ChatEffectiveChannelAccessRead | null;
    instructions: string;
    ownAvailable?: boolean;
    markDefault?: boolean;
    /** Which inheriting scope this is, for the source line; ignored at the app scope. */
    scope?: ChatInheritingAgentScope;
    /** Unique prefix for the instructions field id. */
    idPrefix: string;
    onSave: (value: ChatAgentScopeValue) => Promise<void>;
  } = $props();

  const localization = getLocalization();
  const { t } = localization;
  /** The saved instructions the draft started from, so a reread replaces an untouched draft and keeps an edited one. */
  let base = $state(untrack(() => instructions));
  let draft = $state(untrack(() => instructions));
  let pending = $state(false);
  let error = $state<string | null>(null);

  const shownAccess = $derived(access ?? effective?.access ?? "ask");
  const options = $derived(chatAccessOptions(t, { ownAvailable, markDefault }));
  const description = $derived(options.find((option) => option.value === shownAccess)?.description ?? "");
  const sourceLine = $derived(effective ? chatScopeSourceLine(t, scope, access, effective.source) : null);
  const dirty = $derived(draft !== instructions);
  const tooLong = $derived(draft.length > CHAT_AGENT_INSTRUCTIONS_LIMIT);

  $effect(() => {
    const saved = instructions;
    untrack(() => {
      if (draft === base) draft = saved;
      base = saved;
    });
  });

  async function save(value: ChatAgentScopeValue): Promise<boolean> {
    if (pending) return false;
    pending = true;
    error = null;
    try {
      await onSave(value);
      return true;
    } catch (cause: unknown) {
      error = chatErrorMessage(cause, t("chat.agentAccess.saveFailed"));
      return false;
    } finally {
      pending = false;
    }
  }

  function chooseAccess(value: string): void {
    const next = options.find((option) => option.value === value && !option.disabled)?.value;
    if (!next || next === access) return;
    void save({ access: next, instructions });
  }

  async function saveInstructions(): Promise<void> {
    if (!dirty || tooLong) return;
    const text = draft;
    if (await save({ access, instructions: text })) base = text;
  }

  function cancelInstructions(): void {
    draft = instructions;
  }
</script>

<div class="grid gap-3">
  <div class="grid gap-1">
    <span class="scope-label">{t("chat.agentAccess.access")}</span>
    <div class="flex min-w-0 items-center gap-1.5">
      <Select inline class="min-w-0 flex-1" value={shownAccess} {options} ariaLabel={t("chat.agentAccess.access")} disabled={pending} onChange={chooseAccess} />
      {#if effective}
        <ResetButton canReset={access !== null && !pending} label={t("chat.agentAccess.resetToInherited")} onReset={() => void save({ access: null, instructions })} />
      {/if}
    </div>
    <p class="scope-detail">{description}</p>
    {#if sourceLine}<p class="scope-detail">{sourceLine}</p>{/if}
    {#if !ownAvailable}<p class="scope-detail">{t("chat.agentAccess.ownUnavailable")}</p>{/if}
  </div>

  <div class="grid gap-1">
    <label class="scope-label" for={`${idPrefix}-instructions`}>{t("chat.agentAccess.instructions")}</label>
    <textarea
      id={`${idPrefix}-instructions`}
      class="field min-h-16 resize-y text-[0.8rem]"
      rows="3"
      bind:value={draft}
      placeholder={t("chat.agentAccess.instructionsPlaceholder")}
      disabled={pending}
    ></textarea>
    <p class="scope-detail">{tooLong ? t("chat.agentAccess.instructionsTooLong", formatNumber(localization.locale, CHAT_AGENT_INSTRUCTIONS_LIMIT)) : t("chat.agentAccess.instructionsHint")}</p>
    {#if dirty}
      <div class="flex justify-end gap-1.5">
        <button type="button" class="scope-button" disabled={pending} onclick={cancelInstructions}>{t("chat.agentAccess.cancel")}</button>
        <button type="button" class="scope-button primary" disabled={pending || tooLong} onclick={() => void saveInstructions()}>{t("chat.agentAccess.save")}</button>
      </div>
    {/if}
  </div>

  {#if error}<Notice class="text-panel-detail" message={error} copy={false} />{/if}
</div>

<style>
  .scope-label { color: var(--muted-foreground); font-size: var(--panel-detail-font-size); font-weight: 500; line-height: 1.4; }
  .scope-detail { margin: 0; color: var(--muted-foreground); font-size: var(--panel-detail-font-size); line-height: 1.4; overflow-wrap: anywhere; }
  .scope-button { min-height: 1.9rem; border: 1px solid var(--border); border-radius: var(--floating-item-radius); padding: 0.2rem 0.7rem; font-size: var(--panel-detail-font-size); font-weight: 600; color: var(--foreground); }
  .scope-button:hover:not(:disabled) { background: var(--accent); }
  .scope-button.primary { border-color: transparent; background: var(--primary); color: var(--primary-foreground); }
  .scope-button:disabled { cursor: not-allowed; opacity: 0.5; }
  @media (pointer: coarse) { .scope-button { min-height: 2.75rem; } }
</style>
