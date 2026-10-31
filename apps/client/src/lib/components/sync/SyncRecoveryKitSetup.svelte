<script lang="ts">
  import { onMount } from "svelte";
  import {
    confirmSyncRecoveryKitSaved,
    createSyncRecoveryKit,
    getSyncRecoveryKitState,
    type SyncRecoveryKitState,
  } from "$lib/api/sync";
  import ConfirmDialog from "$lib/components/ui/ConfirmDialog.svelte";
  import Notice from "$lib/components/ui/Notice.svelte";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import { onActiveVaultIdentityChange } from "$lib/vault/active-vault";
  import SyncRecoveryKitSheet from "./SyncRecoveryKitSheet.svelte";

  /**
   * Creates the recovery kit of a vault that is not linked yet. The vault keeps the kit's public
   * keys, its first link starts the sync space with them, and its encrypted backups open with it.
   */
  let {
    buttonClass,
    exportable = true,
  }: {
    buttonClass: string;
    /** Whether the kit sheet offers its printable page and text file export. */
    exportable?: boolean;
  } = $props();

  const { t } = getLocalization();
  let kitState = $state<SyncRecoveryKitState | null>(null);
  let busy = $state(false);
  let failure = $state("");
  let replacing = $state(false);
  let generation = 0;

  async function load(): Promise<void> {
    const expected = ++generation;
    try {
      const next = await getSyncRecoveryKitState();
      if (expected === generation) kitState = next;
    } catch (cause: unknown) {
      if (expected !== generation) return;
      console.warn("recovery kit state could not be loaded", cause);
      failure = t("sync.kit.loadFailed");
    }
  }

  onMount(() => {
    void load();
    return onActiveVaultIdentityChange(() => {
      kitState = null;
      failure = "";
      void load();
    });
  });

  async function create(): Promise<void> {
    replacing = false;
    if (busy) return;
    busy = true;
    failure = "";
    try {
      await createSyncRecoveryKit();
    } catch (cause: unknown) {
      console.warn("recovery kit could not be created", cause);
      failure = t("sync.kit.createFailed");
    } finally {
      busy = false;
      await load();
    }
  }

  async function confirmSaved(lastGroup: string): Promise<boolean> {
    try {
      return await confirmSyncRecoveryKitSaved(lastGroup);
    } finally {
      await load();
    }
  }
</script>

{#if kitState}
  <div class="flex min-w-0 flex-col gap-2">
    <div class="px-1">
      <div class="text-[0.866667rem] text-foreground">{t("sync.kit.heading")}</div>
      <div class="mt-0.5 text-[0.8rem] text-muted-foreground">{t("sync.kit.description")}</div>
    </div>
    {#if kitState.pending}
      <SyncRecoveryKitSheet kit={kitState.pending} {buttonClass} {exportable} onConfirm={confirmSaved} />
    {:else}
      <p class="px-1 text-[0.8rem] leading-5 text-muted-foreground">
        {kitState.stored ? t("sync.kit.createStored") : t("sync.kit.createDescription")}
      </p>
      <div class="flex flex-wrap gap-2">
        <button
          type="button"
          class={buttonClass}
          disabled={busy}
          onclick={() => { if (kitState?.stored) replacing = true; else void create(); }}
        >
          {kitState.stored ? t("sync.kit.replace") : t("sync.kit.create")}
        </button>
      </div>
    {/if}
    {#if failure}
      <Notice class="px-1 text-[0.8rem] leading-5" message={failure} copy={false} />
    {/if}
  </div>
{:else if failure}
  <Notice class="px-1 text-[0.8rem] leading-5" message={failure} copy={false} />
{/if}

{#if replacing}
  <ConfirmDialog
    title={t("sync.kit.replaceTitle")}
    message={t("sync.kit.replaceMessage")}
    confirmLabel={t("sync.kit.replace")}
    cancelLabel={t("common.cancel")}
    onConfirm={() => void create()}
    onCancel={() => { replacing = false; }}
  />
{/if}
