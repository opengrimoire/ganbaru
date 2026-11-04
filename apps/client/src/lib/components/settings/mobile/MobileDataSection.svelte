<script lang="ts">
  import DatabaseRollbacks from "$lib/components/vault/DatabaseRollbacks.svelte";
  import ReplacedVaultBackups from "$lib/components/vault/ReplacedVaultBackups.svelte";
  import VaultBackupRows from "$lib/components/vault/VaultBackupRows.svelte";
  import VaultLockWarning from "$lib/components/vault/VaultLockWarning.svelte";
  import VaultHandoffPanel from "$lib/components/vault/handoff/VaultHandoffPanel.svelte";
  import { getLocalization } from "$lib/i18n/translator.svelte";

  const { t } = getLocalization();
  const actionButtonClass =
    "inline-flex h-7 shrink-0 items-center justify-center rounded-md border border-primary bg-primary px-3 text-[0.8rem] font-medium text-primary-foreground transition-colors active:bg-primary/90 disabled:pointer-events-none disabled:opacity-55";
</script>

<div class="flex flex-col gap-6">
  <VaultLockWarning class="px-1 text-[0.8rem]" />

  <VaultHandoffPanel platform="android" onActivated={() => window.location.reload()} />

  <div class="h-px shrink-0 scale-y-50 bg-border" aria-hidden="true"></div>

  <section class="flex flex-col gap-4">
    <h2 class="px-1 text-[0.866667rem] font-semibold text-foreground">
      {t("settings.data.backupHeading")}
    </h2>

    <VaultBackupRows
      platform="android"
      buttonClass={actionButtonClass}
      onRestored={() => window.location.reload()}
    />
  </section>

  <DatabaseRollbacks onRestored={() => window.location.reload()} />

  <ReplacedVaultBackups onRestored={() => window.location.reload()} />
</div>
