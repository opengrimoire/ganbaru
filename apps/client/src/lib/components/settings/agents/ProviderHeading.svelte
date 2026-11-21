<script module lang="ts">
  /** How a provider or provider family reads at a glance: the color of its state dot. */
  export type ProviderDisplayState = "healthy" | "attention" | "disabled" | "unconfigured";
</script>

<script lang="ts">
  import KeyRound from "@lucide/svelte/icons/key-round";
  import Mail from "@lucide/svelte/icons/mail";
  import UserRound from "@lucide/svelte/icons/user-round";
  import type { Snippet } from "svelte";
  import type { ProviderInstanceRead } from "$lib/chat/contracts";
  import ChatModelAvatar from "$lib/components/chat/identity/ChatModelAvatar.svelte";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import ProviderPlanLimitRings from "./ProviderPlanLimitRings.svelte";

  /**
   * A provider's identity line: its avatar, name, and state dot, then the CLI it runs with the account, plan limits,
   * and sign-in kind as icons whose tooltips hold the details, and `actions` at the end.
   */
  let {
    familyId,
    familyName,
    title,
    state,
    stateLabel,
    subtitle,
    provider,
    avatarSize = 38,
    actions,
  }: {
    familyId: string;
    /** The family's display name, which the avatar announces. */
    familyName: string;
    title: string;
    state: ProviderDisplayState;
    /** What the state dot means, shown in its tooltip. */
    stateLabel: string;
    /** The CLI name, with its version once a probe read it. */
    subtitle: string;
    /** The configuration whose last probe supplies the account, plan limits, and sign-in kind, if any. */
    provider: ProviderInstanceRead | null;
    avatarSize?: number;
    actions?: Snippet;
  } = $props();

  const { t } = getLocalization();
  const probe = $derived(provider?.lastProbe ?? null);
</script>

<div class="provider-heading">
  <ChatModelAvatar {familyId} label={familyName} size={avatarSize} />
  <div class="provider-heading-title">
    <div class="provider-heading-identity">
      <div class="label-line">
        <h3>{title}</h3>
        <span class="state" data-state={state} aria-label={stateLabel} data-app-tooltip={stateLabel}><i></i></span>
      </div>
      <div class="subtitle-line">
        <p>{subtitle}</p>
        {#if probe?.accountLabel}
          <button
            type="button"
            class="detail-icon"
            aria-label={`${t("settings.agents.providers.account")}: ${probe.accountLabel}`}
            data-app-tooltip={probe.accountLabel}
            data-app-tooltip-keep-on-click="true"
          ><Mail size={12} strokeWidth={1.8} /></button>
        {/if}
        {#if provider}<ProviderPlanLimitRings {provider} />{/if}
        {#if probe?.authSource}
          {@const authSourceLabel = probe.authSource === "api_key"
            ? t("settings.agents.providers.authSourceApiKey")
            : t("settings.agents.providers.authSourceAccountLogin")}
          <button
            type="button"
            class="detail-icon"
            aria-label={`${t("settings.agents.providers.authSource")}: ${authSourceLabel}`}
            data-app-tooltip={authSourceLabel}
            data-app-tooltip-keep-on-click="true"
          >
            {#if probe.authSource === "api_key"}
              <KeyRound size={12} strokeWidth={1.8} />
            {:else}
              <UserRound size={12} strokeWidth={1.8} />
            {/if}
          </button>
        {/if}
      </div>
    </div>
    {#if actions}<div class="provider-heading-actions">{@render actions()}</div>{/if}
  </div>
</div>

<style>
  .provider-heading { display:grid; min-width:0; grid-template-columns:auto minmax(0,1fr); align-items:center; gap:0.65rem; padding-inline:0.25rem; }
  .provider-heading-title { display:grid; min-width:0; grid-template-columns:minmax(0,1fr) auto; align-items:center; gap:0.75rem; }
  .provider-heading-identity { display:grid; min-width:0; }
  .label-line,.subtitle-line { display:flex; min-width:0; align-items:center; }
  .label-line { gap:0.3rem; }
  .subtitle-line { gap:0.65rem; }
  h3 { overflow:hidden; font-size:calc(0.833333rem * var(--type-scale)); font-weight:600; text-overflow:ellipsis; white-space:nowrap; }
  p { overflow:hidden; min-width:0; margin-top:0.08rem; color:var(--muted-foreground); font-size:calc(0.7rem * var(--type-scale)); text-overflow:ellipsis; white-space:nowrap; }
  .state { display:inline-grid; width:0.7rem; height:0.7rem; flex:0 0 auto; place-items:center; }
  .state i { width:0.38rem; height:0.38rem; flex:0 0 auto; border-radius:50%; background:var(--status-tentative); }
  .state[data-state="healthy"] i { background:var(--action-confirm-signal); }
  .state[data-state="disabled"] i { background:var(--muted-foreground); opacity:0.7; }
  .state[data-state="unconfigured"] i { background:var(--destructive); }
  .detail-icon { display:inline-grid; width:1rem; height:1rem; flex:0 0 auto; place-items:center; border-radius:0.2rem; color:var(--muted-foreground); }
  .detail-icon:hover,.detail-icon:focus-visible { color:var(--foreground); }
  .provider-heading-actions { display:flex; flex-shrink:0; align-items:center; gap:0.3rem; }
</style>
