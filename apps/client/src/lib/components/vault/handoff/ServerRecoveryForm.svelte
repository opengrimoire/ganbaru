<script lang="ts">
  import { recoverVaultThroughServer } from "$lib/api/vault-handoff";
  import Notice from "$lib/components/ui/Notice.svelte";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import { serverRecoveryError } from "$lib/vault/server-recovery";

  /**
   * Recovers the vault an anchor server runs onto this new desktop with the server's setup code and
   * the recovery kit. The kit leaves the field when it is submitted and is kept nowhere else.
   */
  let {
    buttonClass,
    onActivated,
  }: {
    buttonClass: string;
    /** Called once the recovered vault is the active vault. */
    onActivated: () => void;
  } = $props();

  const { t } = getLocalization();
  let open = $state(false);
  let busy = $state(false);
  let code = $state("");
  let kit = $state("");
  let error = $state<string | null>(null);
  let notice = $state<string | null>(null);
  const canSubmit = $derived(!busy && code.trim() !== "" && kit.trim() !== "");

  async function recover(event: SubmitEvent): Promise<void> {
    event.preventDefault();
    if (!canSubmit) return;
    const typedKit = kit;
    kit = "";
    busy = true;
    error = null;
    notice = null;
    try {
      const outcome = await recoverVaultThroughServer(code.trim(), typedKit);
      if (outcome.inProgress) {
        notice = t("vaultHandoff.copyInProgress");
        return;
      }
      code = "";
      if (outcome.activated) onActivated();
    } catch (cause) {
      error = serverRecoveryError(cause, t);
    } finally {
      busy = false;
    }
  }
</script>

{#if open}
  <form class="flex min-w-0 flex-col gap-2 px-1" onsubmit={(event) => void recover(event)}>
    <p class="text-[0.8rem] leading-5 text-muted-foreground">{t("vaultHandoff.serverRecovery.description")}</p>
    <label class="text-[0.8rem] text-muted-foreground" for="server-recovery-code">{t("vaultHandoff.serverRecovery.codeLabel")}</label>
    <input
      id="server-recovery-code"
      class="field h-9 w-full px-3 font-mono text-sm"
      dir="ltr"
      autocomplete="off"
      autocapitalize="off"
      spellcheck="false"
      placeholder="ganbaru-anchor:…"
      bind:value={code}
      disabled={busy}
    />
    <label class="text-[0.8rem] text-muted-foreground" for="server-recovery-kit">{t("vaultHandoff.serverRecovery.kitLabel")}</label>
    <input
      id="server-recovery-kit"
      class="field h-9 w-full px-3 font-mono text-sm"
      dir="ltr"
      autocomplete="off"
      autocapitalize="off"
      spellcheck="false"
      bind:value={kit}
      disabled={busy}
    />
    <div class="flex flex-wrap gap-2">
      <button type="submit" class={buttonClass} disabled={!canSubmit}>
        {busy ? t("vaultHandoff.serverRecovery.recovering") : t("vaultHandoff.serverRecovery.action")}
      </button>
    </div>
    {#if error}
      <Notice class="text-[0.8rem] leading-5" message={error} copy={false} />
    {:else if notice}
      <p role="status" class="text-[0.8rem] leading-5 text-muted-foreground">{notice}</p>
    {/if}
  </form>
{:else}
  <button
    type="button"
    class="self-center text-[0.8rem] text-muted-foreground underline underline-offset-2"
    onclick={() => { open = true; }}
  >
    {t("vaultHandoff.serverRecovery.open")}
  </button>
{/if}
