<script lang="ts">
  import TriangleAlert from "@lucide/svelte/icons/triangle-alert";
  import Notice from "$lib/components/ui/Notice.svelte";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import { getVaultLockStatus, type VaultLockStatus } from "$lib/vault/state";

  /**
   * Warns that the active vault cannot be fenced against another Ganbaru app because its file system refuses locks.
   * Renders nothing while the vault is held exclusively or the status cannot be read.
   */
  let { class: className }: { class?: string } = $props();

  const { t } = getLocalization();
  let status = $state<VaultLockStatus | null>(null);

  async function loadStatus(): Promise<void> {
    try {
      status = await getVaultLockStatus();
    } catch (cause: unknown) {
      console.error("Read vault lock status", cause);
      status = null;
    }
  }

  $effect(() => {
    void loadStatus();
  });
</script>

{#if status === "unsupported"}
  <Notice
    class={className}
    message={t("sync.lockUnsupported")}
    icon={TriangleAlert}
    tone="warning"
    role="status"
    copy={false}
  />
{/if}
