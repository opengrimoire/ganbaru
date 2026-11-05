<script lang="ts">
  import ArrowRight from "@lucide/svelte/icons/arrow-right";
  import LoaderCircle from "@lucide/svelte/icons/loader-circle";
  import Monitor from "@lucide/svelte/icons/monitor";
  import ScanLine from "@lucide/svelte/icons/scan-line";
  import Server from "@lucide/svelte/icons/server";
  import Settings from "@lucide/svelte/icons/settings";
  import Smartphone from "@lucide/svelte/icons/smartphone";
  import { onMount, untrack } from "svelte";
  import { getSyncServerStatus, type SyncServerView } from "$lib/api/sync";
  import ConfirmDialog from "$lib/components/ui/ConfirmDialog.svelte";
  import Loading from "$lib/components/ui/Loading.svelte";
  import Notice from "$lib/components/ui/Notice.svelte";
  import { cn } from "$lib/utils";
  import {
    cancelDesktopBundleReceive,
    createPairingInvitation,
    getCachedPairingStatus,
    grantDesktopNetworkAccess,
    enrollWithDesktop,
    readEnrollmentCode,
    readSuggestedDeviceLabel,
    readPairingStatus,
    pickLinkFolder,
    recoverLocalVaultCopy,
    requestVaultBootstrap,
    resetLinkFolder,
    respondToEnrollment,
    revokeDesktopNetworkAccess,
    unlinkVaultDevice,
    type PairingInvitation,
    type PairingStatus,
    type DesktopNetworkAccess,
  } from "$lib/api/vault-handoff";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import SwitchField from "$lib/components/ui/SwitchField.svelte";
  import SyncRecoveryKitSetup from "$lib/components/sync/SyncRecoveryKitSetup.svelte";
  import SyncServer from "$lib/components/sync/SyncServer.svelte";
  import SyncSection from "$lib/components/sync/SyncSection.svelte";
  import {
    formatHandoffError,
    formatPairingCodeError,
    hasNewLinkedDevice,
  } from "$lib/vault/handoff-workflow";
  import EnrollmentApproval from "./EnrollmentApproval.svelte";
  import EnrollmentCodeWaiting from "./EnrollmentCodeWaiting.svelte";
  import LinkedDeviceSuccess from "./LinkedDeviceSuccess.svelte";
  import MobilePairingScanner from "./MobilePairingScanner.svelte";
  import MobileServerConnect from "./MobileServerConnect.svelte";

  type BusyAction = "status" | "invite" | "network" | "pair" | "approval" | "bootstrap" | "unlink" | "recover" | "linkFolder";

  /** How long onboarding waits for an approved device to appear before it shows a new code. */
  const APPROVED_ENROLLMENT_SETTLE_ATTEMPTS = 20;
  const APPROVED_ENROLLMENT_SETTLE_INTERVAL_MS = 500;

  let {
    platform,
    presentation = "settings",
    initialInvitation = null,
    initialStatus = null,
    bootstrapReplicaOnLink = false,
    onActivated,
    onStatusChange,
    onOpenDataSettings,
  }: {
    platform: "desktop" | "android";
    presentation?: "settings" | "onboarding" | "control";
    initialInvitation?: PairingInvitation | null;
    initialStatus?: PairingStatus | null;
    bootstrapReplicaOnLink?: boolean;
    onActivated?: () => void;
    onStatusChange?: (status: PairingStatus) => void;
    onOpenDataSettings?: () => void;
  } = $props();

  const { t } = getLocalization();
  const desktopQrAvailable = __GANBARU_BUILD_PLATFORM__ !== "android";
  let status = $state<PairingStatus | null>(
    untrack(() => initialStatus ?? getCachedPairingStatus() ?? null),
  );
  /** The vault's server, listed with the devices in the sync control. */
  let server = $state<SyncServerView | null>(null);
  let invitation = $state<PairingInvitation | null>(untrack(() => initialInvitation));
  let deviceIdsAtInvitation = new Set(
    untrack(() => status?.devices.map((device) => device.deviceId) ?? []),
  );
  let scanning = $state(false);
  /** Whether this phone is connecting to an anchor server instead of a computer. */
  let serverConnecting = $state(false);
  let codeDialogVisible = $state(false);
  let qrDialogVisible = $state(false);
  let scannerVersion = $state(0);
  let busy = $state<BusyAction | null>(untrack(() => status ? null : "status"));
  let statusRefreshing = false;
  let notice = $state<string | null>(null);
  let error = $state<string | null>(null);
  let codeError = $state<string | null>(null);
  let networkError = $state<string | null>(null);
  let approvalError = $state<string | null>(null);
  let enrollmentCode = $state<string | null>(null);
  let confirmation = $state<"network" | "networkRevoke" | "unlink" | "recover" | "firstCopy" | null>(null);
  let unlinkDeviceId = $state<string | null>(null);
  /** The folder chosen for the first copy of the linked vault on this computer; null uses the default. */
  let linkFolder = $state<string | null>(null);
  let networkAccess = $state<DesktopNetworkAccess | null>(
    untrack(() => initialInvitation?.networkAccess ?? status?.networkAccess ?? null),
  );
  let networkStepsVisible = $state(
    untrack(() => {
      const state = initialInvitation?.networkAccess?.state ?? status?.networkAccess?.state;
      return state === "authorizationRequired"
        || state === "manualActionRequired"
        || state === "networkConfirmationRequired";
    }),
  );

  const buttonClass =
    "inline-flex min-h-8 items-center justify-center gap-1.5 rounded-md border border-border px-3 text-[0.8rem] font-medium transition-colors hover:bg-accent disabled:pointer-events-none disabled:opacity-55";
  const primaryButtonClass = `${buttonClass} border-primary bg-primary text-primary-foreground hover:bg-primary/90`;
  const controlButtonClass = $derived(
    `flex min-w-0 w-full items-center justify-between gap-4 overflow-hidden px-3 text-left text-sm transition-colors ${platform === "android" ? "min-h-12 active:bg-accent" : "py-1.5 hover:bg-accent"} disabled:cursor-not-allowed disabled:text-muted-foreground/50`,
  );
  const desktopNetworkAccessSetting = $derived(
    presentation === "settings"
      && platform === "desktop"
      && __GANBARU_BUILD_PLATFORM__ === "linux"
      && networkAccess !== null,
  );
  const controlsBusy = $derived(
    busy !== null && !(presentation === "settings" && busy === "invite"),
  );
  const networkAccessEnabled = $derived(
    !desktopNetworkAccessSetting
      || networkAccess?.state === "granted"
      || networkAccess?.state === "notRequired",
  );
  /** Whether this desktop must allow phone connections through the firewall before another device can link. */
  const networkAccessBlocksLinking = $derived(
    desktopNetworkAccessSetting
      && (networkAccess?.state === "authorizationRequired" || networkAccess?.state === "networkConfirmationRequired"),
  );
  /** Bumped to replay the flash on the Local network access setting, which re-creates the row. */
  let networkFlash = $state(0);
  const networkAccessDescription = $derived(
    networkAccess?.state === "notRequired"
      ? t("vaultHandoff.networkAccessNotRequired")
      : networkAccess?.state === "manualActionRequired"
        ? t("vaultHandoff.networkAccessManual")
        : networkAccess?.state === "networkConfirmationRequired"
          ? t("vaultHandoff.networkAccessNetworkConfirmation")
          : t("vaultHandoff.networkAccessAllowed"),
  );
  const isCoordinatorClient = $derived(status?.coordinatorEndpoint != null);
  /** A linked device that has not copied the linked vault yet offers that first copy. */
  const firstCopyPending = $derived(
    status?.linked === true && status.replicaReady === false && isCoordinatorClient,
  );
  /** A linked device whose copy is ready can keep it as a vault of its own. */
  const recoverAvailable = $derived(status?.replicaReady === true && isCoordinatorClient);
  /** Failures that replace the content they block render in that place instead of the trailing error line. */
  const errorShownInPlace = $derived(
    error !== null
      && (!status
        || (!status.linked && platform === "desktop" && presentation === "onboarding" && !invitation)),
  );

  function fail(cause: unknown): void {
    error = formatHandoffError(cause, t);
  }

  function applyNetworkAccess(next: DesktopNetworkAccess): void {
    networkAccess = next;
    if (
      next.state === "authorizationRequired"
      || next.state === "manualActionRequired"
      || next.state === "networkConfirmationRequired"
    ) {
      networkStepsVisible = true;
    }
  }

  async function loadStatus(): Promise<PairingStatus> {
    const next = await readPairingStatus();
    status = next;
    if (next.networkAccess) applyNetworkAccess(next.networkAccess);
    if (presentation === "settings" && platform === "desktop") {
      if (next.pendingEnrollment) {
        qrDialogVisible = true;
      } else if (qrDialogVisible && !invitation && busy !== "invite" && !error) {
        // A dialog opened only for a request that ended without an answer has nothing left to show.
        qrDialogVisible = false;
      }
    }
    onStatusChange?.(next);
    return next;
  }

  /** Answers the waiting enrollment; a decline spends the invitation, so a fresh one replaces it. */
  async function respondToPendingEnrollment(approve: boolean): Promise<void> {
    const pending = status?.pendingEnrollment;
    if (!pending || busy !== null) return;
    busy = "approval";
    error = null;
    approvalError = null;
    notice = null;
    let answered = false;
    try {
      await respondToEnrollment(pending.requestId, approve);
      answered = true;
    } catch (cause) {
      approvalError = formatHandoffError(cause, t);
    } finally {
      busy = null;
    }
    if (answered && !approve) {
      invitation = null;
      await showInvitation();
    } else if (answered) {
      invitation = null;
      if (presentation === "settings") qrDialogVisible = false;
    }
    try {
      await loadStatus();
    } catch (cause) {
      fail(cause);
    }
    if (answered && approve && presentation === "onboarding") await settleApprovedEnrollment();
  }

  /** Gives an approved device time to finish linking before onboarding offers a new code. */
  async function settleApprovedEnrollment(): Promise<void> {
    for (let attempt = 0; attempt < APPROVED_ENROLLMENT_SETTLE_ATTEMPTS; attempt += 1) {
      if (status?.linked || (status && hasNewLinkedDevice(deviceIdsAtInvitation, status.devices))) return;
      await new Promise<void>((resolve) => window.setTimeout(resolve, APPROVED_ENROLLMENT_SETTLE_INTERVAL_MS));
      try {
        await loadStatus();
      } catch (cause) {
        fail(cause);
        return;
      }
    }
    if (!status?.linked && !invitation && busy === null) await showInvitation();
  }

  /** Closing the dialog while a device waits declines it rather than leaving it to time out. */
  function closeInvitationDialog(): void {
    if (busy === "approval") return;
    if (status?.pendingEnrollment) {
      // A failed decline means the request already ended, which is the outcome closing asks for.
      void respondToEnrollment(status.pendingEnrollment.requestId, false)
        .catch(() => undefined)
        .finally(() => void loadStatus().catch(() => undefined));
    }
    qrDialogVisible = false;
    invitation = null;
    approvalError = null;
  }

  async function refreshStatus(): Promise<void> {
    if (statusRefreshing) return;
    statusRefreshing = true;
    const showLoadingState = status === null;
    if (showLoadingState) busy = "status";
    error = null;
    try {
      await loadStatus();
    } catch (cause) {
      fail(cause);
    } finally {
      statusRefreshing = false;
      if (showLoadingState) busy = null;
    }
  }

  async function showInvitation(): Promise<void> {
    busy = "invite";
    error = null;
    notice = null;
    approvalError = null;
    try {
      const nextInvitation = await createPairingInvitation();
      // A new invitation cancels any request that used the previous one.
      if (status?.pendingEnrollment) status = { ...status, pendingEnrollment: null };
      deviceIdsAtInvitation = new Set(status?.devices.map((device) => device.deviceId) ?? []);
      invitation = nextInvitation;
      if (invitation.networkAccess) applyNetworkAccess(invitation.networkAccess);
    } catch (cause) {
      fail(cause);
    } finally {
      busy = null;
    }
  }

  /**
   * Opens the pairing QR code, or flashes the Local network access setting when the firewall would keep the other
   * device out, since the code cannot work until that is allowed.
   */
  function openInvitationDialog(): void {
    if (networkAccessBlocksLinking) {
      networkFlash += 1;
      return;
    }
    qrDialogVisible = true;
    if (busy === "invite") return;
    invitation = null;
    error = null;
    void showInvitation();
  }

  async function updateNetworkAccess(grant: boolean): Promise<void> {
    busy = "network";
    error = null;
    networkError = null;
    notice = null;
    try {
      applyNetworkAccess(
        grant
          ? await grantDesktopNetworkAccess()
          : await revokeDesktopNetworkAccess(),
      );
      notice = grant ? null : t("vaultHandoff.networkAccessRevoked");
    } catch (cause) {
      networkError = formatHandoffError(cause, t);
    } finally {
      busy = null;
    }
  }

  async function grantNetworkAccess(): Promise<void> {
    confirmation = null;
    await updateNetworkAccess(true);
  }

  async function revokeNetworkAccess(): Promise<void> {
    confirmation = null;
    await updateNetworkAccess(false);
  }

  function setNetworkAccess(enabled: boolean): void {
    if (enabled) confirmation = "network";
    else confirmation = "networkRevoke";
  }

  async function acceptInvitation(encoded: string): Promise<void> {
    const fromCodeDialog = platform === "desktop" && codeDialogVisible;
    busy = "pair";
    error = null;
    codeError = null;
    notice = null;
    try {
      const deviceLabel = await readSuggestedDeviceLabel();
      enrollmentCode = await readEnrollmentCode(encoded);
      await enrollWithDesktop(encoded, deviceLabel);
      scanning = false;
      const next = await loadStatus();
      if (
        platform === "android"
        && presentation === "onboarding"
        && bootstrapReplicaOnLink
        && !next.replicaReady
      ) {
        const outcome = await requestVaultBootstrap();
        if (outcome.inProgress) throw new Error("initial linked vault activation is already running");
        await loadStatus();
      }
      codeDialogVisible = false;
      codeError = null;
    } catch (cause) {
      if (fromCodeDialog) {
        codeError = formatPairingCodeError(cause, t);
      } else {
        fail(cause);
        if (platform === "android") {
          scannerVersion += 1;
          scanning = true;
        }
      }
    } finally {
      enrollmentCode = null;
      busy = null;
    }
  }

  /** Copies the linked vault to this device for the first time. */
  async function copyFirst(): Promise<void> {
    confirmation = null;
    busy = "bootstrap";
    error = null;
    notice = null;
    try {
      const outcome = await requestVaultBootstrap();
      if (outcome.inProgress) {
        notice = t("vaultHandoff.copyInProgress");
        return;
      }
      await loadStatus();
      notice = t("vaultHandoff.copied");
      if (outcome.activated) onActivated?.();
    } catch (cause) {
      fail(cause);
    } finally {
      busy = null;
    }
  }

  async function unlink(): Promise<void> {
    confirmation = null;
    busy = "unlink";
    error = null;
    try {
      if (!unlinkDeviceId) throw new Error("Linked device identity is unavailable");
      await unlinkVaultDevice(unlinkDeviceId);
      await loadStatus();
      invitation = null;
      notice = t("vaultHandoff.unlinked");
    } catch (cause) {
      fail(cause);
    } finally {
      unlinkDeviceId = null;
      busy = null;
    }
  }

  async function recover(): Promise<void> {
    confirmation = null;
    busy = "recover";
    error = null;
    try {
      await recoverLocalVaultCopy();
      await loadStatus();
      notice = t("vaultHandoff.recovered");
      onActivated?.();
    } catch (cause) {
      fail(cause);
    } finally {
      busy = null;
    }
  }

  async function cancelReceive(): Promise<void> {
    try {
      await cancelDesktopBundleReceive();
    } catch (cause) {
      fail(cause);
    }
  }

  /** Starts the first copy after confirmation, or cancels the copy that is running. */
  function handleFirstCopyAction(): void {
    if (busy === "bootstrap") {
      void cancelReceive();
      return;
    }
    confirmation = "firstCopy";
  }

  /** Lets the user choose an empty folder for the first copy of the linked vault on this computer. */
  async function chooseLinkFolder(): Promise<void> {
    busy = "linkFolder";
    error = null;
    try {
      const chosen = await pickLinkFolder();
      if (chosen !== null) linkFolder = chosen;
    } catch (cause) {
      fail(cause);
    } finally {
      busy = null;
    }
  }

  async function useDefaultLinkFolder(): Promise<void> {
    busy = "linkFolder";
    error = null;
    try {
      await resetLinkFolder();
      linkFolder = null;
    } catch (cause) {
      fail(cause);
    } finally {
      busy = null;
    }
  }

  /** Copies the linked vault here again after sync reported that this copy can no longer catch up. */
  async function getFreshCopy(): Promise<void> {
    const outcome = await requestVaultBootstrap();
    if (outcome.inProgress) throw new Error("finish or retry the current vault transfer first");
    await loadStatus();
    if (outcome.activated) onActivated?.();
  }

  /** Reads the vault's server for the sync control; without it the control lists devices only. */
  async function loadServer(): Promise<void> {
    try {
      server = await getSyncServerStatus();
    } catch (cause: unknown) {
      console.warn("sync server status could not be loaded", cause);
    }
  }

  onMount(() => {
    if (presentation === "control") void loadServer();
    const prepareOnboarding = (): void => {
      if (presentation !== "onboarding" || status?.linked) return;
      if (platform === "desktop") {
        if (!invitation) void showInvitation();
      } else {
        scanning = true;
      }
    };
    if (status) {
      onStatusChange?.(status);
      prepareOnboarding();
      void refreshStatus();
    } else {
      void refreshStatus().then(prepareOnboarding);
    }
    const timer = window.setInterval(() => {
      if (busy || statusRefreshing) return;
      const invitationCode = invitation?.invitation;
      void loadStatus().then((next) => {
        if (
          invitationCode
          && invitation?.invitation === invitationCode
          && hasNewLinkedDevice(deviceIdsAtInvitation, next.devices)
        ) {
          invitation = null;
        }
      }).catch(() => undefined);
    }, 3_000);
    return () => window.clearInterval(timer);
  });
</script>

{#snippet onboardingLoader()}
  <div
    class="grid min-h-56 place-items-center"
    role="status"
    aria-label={t("common.loading")}
  >
    <LoaderCircle size={24} strokeWidth={1.8} class="animate-spin text-muted-foreground" />
  </div>
{/snippet}

<section class={presentation === "control" ? "" : "flex flex-col gap-4"}>
  {#if presentation === "onboarding" && platform === "desktop" && status?.linked !== true}
    <div class="space-y-2 text-center">
      <h1 class="text-2xl font-semibold leading-tight text-foreground min-[560px]:text-3xl">
        {t("vaultHandoff.desktopOnboardingTitle")}
      </h1>
      <button
        type="button"
        class="text-sm text-muted-foreground hover:text-foreground disabled:opacity-55"
        disabled={busy !== null}
        onclick={() => { codeDialogVisible = true; error = null; codeError = null; }}
      >
        {t("vaultHandoff.linkThisComputer")}
      </button>
    </div>
  {/if}
  {#if presentation === "settings"}
    <h2 class="px-1 text-[0.866667rem] font-semibold text-foreground">{t("vaultHandoff.heading")}</h2>
    {#if desktopNetworkAccessSetting}
      {#key networkFlash}
        <div class={cn("rounded-md", networkFlash > 0 && "attention-flash")}>
          <SwitchField
            label={t("vaultHandoff.networkAccessHeading")}
            description={networkAccessDescription}
            checked={networkAccessEnabled}
            disabled={controlsBusy || networkAccess?.state === "notRequired" || networkAccess?.state === "manualActionRequired"}
            onChange={setNetworkAccess}
          />
        </div>
      {/key}
      {#if networkError}
        <Notice class="px-1 text-[0.8rem] leading-5" message={networkError} />
      {/if}
    {/if}
  {/if}

  {#if busy === "status"}
    {#if presentation === "onboarding"}
      {@render onboardingLoader()}
    {:else}
      <Loading layout="fill" label={t("settings.data.loadingFolder")} />
    {/if}
  {:else if status}
    <fieldset
      disabled={presentation === "settings" && status.linked && status.canInvite && !networkAccessEnabled}
      class={presentation === "settings"
        ? `m-0 flex min-w-0 flex-col gap-3 border-0 p-0 ${status.linked && status.canInvite && !networkAccessEnabled ? "opacity-50" : ""}`
        : "contents"}
    >
    {#if presentation === "settings"}
      <div class="px-1 py-1">
        <div class="text-[0.866667rem] text-foreground">{t("vaultHandoff.devicesHeading")}</div>
        <div class="mt-0.5 text-[0.8rem] text-muted-foreground">{t("vaultHandoff.description")}</div>
      </div>
    {/if}
    {#if presentation === "onboarding" && status.linked}
      {#if bootstrapReplicaOnLink && !status.replicaReady}
        {@render onboardingLoader()}
      {:else}
        <LinkedDeviceSuccess
          device={status.peerLabel ?? t(platform === "android" ? "vaultHandoff.desktopDevice" : "vaultHandoff.androidDevice")}
        />
      {/if}
    {:else}
      {#if presentation === "control" && server}
        <div class={`flex w-full min-w-0 items-center gap-2 px-3 ${platform === "android" ? "min-h-12" : "py-1.5"}`}>
          <Server size={14} strokeWidth={1.8} class="shrink-0 opacity-70" aria-hidden="true" />
          <div class="flex min-w-0 flex-1 items-baseline gap-1 overflow-hidden whitespace-nowrap">
            <div class="min-w-0 truncate text-sm text-foreground">
              {server.mode === "anchor" ? t("vaultHandoff.anchorServer") : t("vaultHandoff.relayServer")}
            </div>
            <span class="shrink-0 text-sm text-foreground">{t("vaultHandoff.linkedLabel")}</span>
          </div>
        </div>
      {/if}
      {#if status.devices.length > 0 && presentation !== "onboarding"}
        {#each status.devices as device (device.deviceId)}
          <div class={presentation === "control"
            ? `flex w-full min-w-0 items-center gap-2 px-3 ${platform === "android" ? "min-h-12" : "py-1.5"}`
            : "flex min-w-0 items-center gap-2 px-1 py-1"}>
            {#if device.kind === "computer"}
              <Monitor size={presentation === "control" ? 14 : 17} strokeWidth={1.8} class="shrink-0 opacity-70" aria-hidden="true" />
            {:else}
              <Smartphone size={presentation === "control" ? 14 : 17} strokeWidth={1.8} class="shrink-0 opacity-70" aria-hidden="true" />
            {/if}
            <div class="flex min-w-0 flex-1 items-baseline gap-1 overflow-hidden whitespace-nowrap">
              <div class={presentation === "control" ? "min-w-0 truncate text-sm text-foreground" : "min-w-0 truncate text-[0.866667rem] font-medium text-foreground"}>
                {device.label ?? (device.kind === "computer" ? t("vaultHandoff.desktopDevice") : t("vaultHandoff.androidDevice"))}
              </div>
              <span class={presentation === "control" ? "shrink-0 text-sm text-foreground" : "shrink-0 text-[0.866667rem] font-medium text-foreground"}>{t("vaultHandoff.linkedLabel")}</span>
            </div>
            {#if presentation === "settings"}
              <button
                type="button"
                class={buttonClass}
                disabled={controlsBusy}
                onclick={() => { unlinkDeviceId = device.deviceId; confirmation = "unlink"; }}
              >
                {t("vaultHandoff.unlink")}
              </button>
            {/if}
          </div>
        {/each}
      {:else}
        {#if presentation === "control"}
          {#if !server}
            <div class={`flex w-full min-w-0 items-center px-3 text-sm text-muted-foreground ${platform === "android" ? "min-h-12" : "h-8"}`}>
              <span class="min-w-0 truncate">{t("vaultHandoff.notLinked")}</span>
            </div>
          {/if}
        {:else if presentation !== "onboarding"}
          <p class="px-1 py-1 text-[0.866667rem] text-muted-foreground">{t("vaultHandoff.notLinked")}</p>
        {/if}
      {/if}
    {/if}

    {#if platform === "desktop" && presentation === "settings" && status.canInvite}
      <div class="flex flex-wrap gap-2">
        <button type="button" class={status.linked ? buttonClass : primaryButtonClass} disabled={controlsBusy} onclick={openInvitationDialog}>
          {status.linked ? t("vaultHandoff.linkAnotherDevice") : t("vaultHandoff.createQr")}
        </button>
        {#if !status.linked}
          <button type="button" class={buttonClass} disabled={busy !== null} onclick={() => { codeDialogVisible = true; error = null; codeError = null; }}>
            {t("vaultHandoff.enterCode")}
          </button>
        {/if}
      </div>
    {:else if !status.linked && platform === "desktop" && presentation === "onboarding" && !invitation && !status.pendingEnrollment}
      {#if error}
        <div class="flex min-h-56">
          <Notice
            layout="fill"
            class="text-sm"
            message={error}
            retryLabel={t("vaultHandoff.retry")}
            onRetry={() => (busy !== null ? undefined : showInvitation())}
          />
        </div>
      {:else}
        {@render onboardingLoader()}
      {/if}
    {:else if !status.linked && platform === "android"}
      {#if enrollmentCode}
        <EnrollmentCodeWaiting code={enrollmentCode} />
      {:else if serverConnecting}
        <MobileServerConnect
          onActivated={() => { serverConnecting = false; onActivated?.(); }}
          onCancel={() => { serverConnecting = false; scannerVersion += 1; scanning = presentation === "onboarding"; }}
        />
      {:else}
        {#if scanning}
          {#key scannerVersion}
            <MobilePairingScanner
              disabled={busy !== null}
              onInvitation={acceptInvitation}
              onError={(cause) => { fail(cause); scanning = false; }}
            />
          {/key}
        {:else}
          <button type="button" class={primaryButtonClass} disabled={busy !== null} onclick={() => { scanning = true; error = null; }}>
            <ScanLine size={14} strokeWidth={2} aria-hidden="true" />
            {t("vaultHandoff.linkToDesktop")}
          </button>
        {/if}
        <button
          type="button"
          class={`${buttonClass} min-h-11`}
          disabled={busy !== null}
          onclick={() => { scanning = false; error = null; serverConnecting = true; }}
        >
          <Server size={14} strokeWidth={2} aria-hidden="true" />
          {t("vaultHandoff.serverConnect.open")}
        </button>
      {/if}
    {/if}

    {#if desktopQrAvailable && platform === "desktop" && (invitation || status.pendingEnrollment) && presentation === "onboarding"}
      <div class={networkStepsVisible
        ? "grid items-start gap-7 min-[800px]:grid-cols-2 min-[800px]:gap-10"
        : "flex flex-col items-center gap-4"}>
        {#if networkStepsVisible}
          <div class="flex flex-col items-center gap-3 px-1 text-center">
            <div class="space-y-1">
              <p class="text-base font-semibold text-foreground">
                {t("vaultHandoff.stepOne")}
              </p>
              <p class="mx-auto max-w-88 text-sm leading-6 text-muted-foreground">
                {t("vaultHandoff.networkAccessDescription")}
              </p>
            </div>
            {#if networkAccess?.state === "authorizationRequired" || networkAccess?.state === "networkConfirmationRequired"}
              <button
                type="button"
                class={`${buttonClass} bg-background`}
                disabled={busy !== null}
                onclick={() => { confirmation = "network"; }}
              >
                {#if busy === "network"}<LoaderCircle size={14} class="animate-spin" />{/if}
                {t("vaultHandoff.allowNetworkAccess")}
              </button>
            {:else if networkAccess?.state === "granted" || networkAccess?.state === "notRequired"}
              <p class="text-[0.8rem] leading-5 text-muted-foreground">
                {t("vaultHandoff.networkAccessGranted")}
              </p>
            {:else if networkAccess?.state === "manualActionRequired"}
              <Notice class="text-[0.8rem] leading-5" tone="warning" copy={false} message={t("vaultHandoff.networkAccessManual")} />
            {/if}
            {#if networkError}
              <Notice class="w-full max-w-88 justify-center text-[0.8rem] leading-5" message={networkError} />
            {/if}
          </div>
        {/if}

        <div class="flex min-w-0 flex-col items-center gap-4">
          {#if status.pendingEnrollment}
            <EnrollmentApproval
              pending={status.pendingEnrollment}
              busy={busy === "approval"}
              onRespond={(approve) => void respondToPendingEnrollment(approve)}
            />
          {:else if invitation}
            <div class="space-y-1 px-1 text-center">
              {#if networkStepsVisible}
                <p class="text-base font-semibold text-foreground">
                  {t("vaultHandoff.stepTwo")}
                </p>
              {/if}
              <p class="mx-auto max-w-sm text-sm leading-6 text-muted-foreground">
                {t("vaultHandoff.desktopOnboardingDescription")}
              </p>
            </div>
            {#await import("./PairingQrCode.svelte") then module}
              {@const PairingQrCode = module.default}
              <PairingQrCode {invitation} onExpired={() => void showInvitation()} />
            {/await}
          {/if}
          {#if approvalError}
            <Notice class="w-full max-w-88 justify-center text-[0.8rem] leading-5" message={approvalError} />
          {/if}
        </div>
      </div>
    {/if}

    {#if presentation === "control" || (status.linked && presentation !== "onboarding")}
      {#if presentation === "settings" && firstCopyPending}
        <p class="px-1 text-[0.8rem] leading-5 text-muted-foreground">{t("vaultHandoff.copyHereDescription")}</p>
        {#if platform === "desktop"}
          <div class="flex flex-wrap items-center justify-between gap-x-4 gap-y-2 px-1">
            <div class="min-w-0 flex-1 text-[0.8rem] leading-5">
              <div class="text-foreground">{t("vaultHandoff.linkFolder")}</div>
              <div class="break-all text-muted-foreground">{linkFolder ?? t("vaultHandoff.linkFolderDefault")}</div>
            </div>
            <div class="flex shrink-0 gap-2">
              {#if linkFolder !== null}
                <button type="button" class={buttonClass} disabled={busy !== null} onclick={() => void useDefaultLinkFolder()}>
                  {t("vaultHandoff.useDefaultLinkFolder")}
                </button>
              {/if}
              <button type="button" class={buttonClass} disabled={busy !== null} onclick={() => void chooseLinkFolder()}>
                {t("vaultHandoff.chooseLinkFolder")}
              </button>
            </div>
          </div>
        {/if}
      {/if}
      {#if firstCopyPending || (presentation === "settings" && recoverAvailable)}
      <div class={presentation === "control"
        ? "flex flex-col"
        : "flex flex-wrap gap-2"}>
        {#if firstCopyPending}
          {#if presentation === "control"}<div class="mx-3 my-1.5 border-t border-border"></div>{/if}
          <button
            type="button"
            class={presentation === "control" ? controlButtonClass : primaryButtonClass}
            disabled={busy !== null && busy !== "bootstrap"}
            onclick={handleFirstCopyAction}
          >
            {#if presentation === "control"}
              <span class="min-w-0 truncate">{busy === "bootstrap" ? t("vaultHandoff.cancel") : t("vaultHandoff.copyHere")}</span>
              {#if busy === "bootstrap"}
                <LoaderCircle size={14} strokeWidth={1.8} class="shrink-0 animate-spin opacity-70" aria-hidden="true" />
              {:else}
                <ArrowRight size={14} strokeWidth={1.8} class="shrink-0 opacity-70" aria-hidden="true" />
              {/if}
            {:else}
              {#if busy === "bootstrap"}<LoaderCircle size={14} class="animate-spin" />{/if}
              {busy === "bootstrap" ? t("vaultHandoff.cancel") : t("vaultHandoff.copyHere")}
            {/if}
          </button>
        {/if}
        {#if presentation === "settings" && recoverAvailable}
          <button type="button" class={buttonClass} disabled={busy !== null} onclick={() => { confirmation = "recover"; }}>
            {t("vaultHandoff.recover")}
          </button>
        {/if}
      </div>
      {/if}
      {#if presentation === "settings"}
        {#if platform === "android" && !status.replicaReady}
          <p class="px-1 text-[0.8rem] leading-5 text-muted-foreground">{t("vaultHandoff.localBackup")}</p>
        {/if}
      {/if}
    {/if}

    </fieldset>
    {#if presentation === "control" && onOpenDataSettings}
      <div class="mx-3 my-1.5 border-t border-border"></div>
      <button type="button" class={`${controlButtonClass} text-muted-foreground hover:text-foreground`} onclick={onOpenDataSettings}>
        <span class="min-w-0 truncate">{t("vaultHandoff.syncSettings")}</span>
        <Settings size={14} strokeWidth={1.8} class="shrink-0 opacity-70" aria-hidden="true" />
      </button>
    {/if}
    {#if presentation === "settings" && !status.linked && !status.serverSync}
      <SyncRecoveryKitSetup {buttonClass} printable={platform === "desktop"} />
      {#if platform === "desktop"}
        <SyncServer {buttonClass} canManage onAdded={() => void refreshStatus()} />
      {/if}
    {/if}
    {#if presentation === "settings" && (status.linked || status.serverSync)}
      <SyncSection
        {buttonClass}
        mobileLayout={platform === "android"}
        onRecoverLocalCopy={recoverAvailable && busy === null ? () => { confirmation = "recover"; } : undefined}
        onFreshCopy={isCoordinatorClient && status.replicaReady ? getFreshCopy : undefined}
      />
    {/if}
  {:else if error}
    {#if presentation === "onboarding"}
      <div class="flex min-h-56">
        <Notice
          layout="fill"
          class="text-sm"
          message={error}
          retryLabel={t("vaultHandoff.retry")}
          onRetry={() => (busy !== null ? undefined : refreshStatus())}
        />
      </div>
    {:else}
      <Notice
        class="px-1 text-[0.8rem] leading-5"
        message={error}
        retryLabel={t("vaultHandoff.retry")}
        onRetry={() => (busy !== null ? undefined : refreshStatus())}
      />
    {/if}
  {/if}

  {#if presentation !== "control" && busy === "bootstrap"}
    <p role="status" class="flex items-center gap-2 px-1 text-[0.8rem] text-muted-foreground">
      <LoaderCircle size={14} strokeWidth={2} class="animate-spin" aria-hidden="true" />
      {t("vaultHandoff.workingCopy")}
    </p>
  {/if}
  {#if status?.revokedByCoordinator && presentation === "settings"}
    <p role="status" class="px-1 text-[0.8rem] leading-5 text-muted-foreground">
      {t("vaultHandoff.revokedByCoordinator")}
    </p>
  {/if}
  {#if notice && presentation !== "control" && presentation !== "onboarding"}<p role="status" class="px-1 text-[0.8rem] leading-5 text-muted-foreground">{notice}</p>{/if}
  {#if error && !errorShownInPlace}<Notice class={`px-1 text-[0.8rem] leading-5 ${presentation === "onboarding" ? "justify-center" : ""}`} message={error} />{/if}
</section>

{#if desktopQrAvailable && platform === "desktop" && qrDialogVisible && presentation === "settings"}
  {#await import("./PairingQrDialog.svelte")}
    <!-- Show the dialog frame immediately while the desktop-only QR dialog module loads. -->
    <!-- svelte-ignore a11y_click_events_have_key_events -->
    <!-- svelte-ignore a11y_no_static_element_interactions -->
    <div class="fixed inset-0 z-90 flex items-center justify-center p-4" onclick={() => { qrDialogVisible = false; }}>
      <div class="surface-backdrop absolute inset-0"></div>
      <!-- svelte-ignore a11y_click_events_have_key_events -->
      <div
        class="relative z-10 flex max-h-full w-full max-w-md flex-col overflow-y-auto surface-dialog px-5 py-5 sm:px-8"
        role="dialog"
        aria-modal="true"
        aria-labelledby="pairing-qr-loading-title"
        tabindex="-1"
        onclick={(event) => event.stopPropagation()}
      >
        <div>
          <div class="flex items-center justify-between gap-3">
            <h2 id="pairing-qr-loading-title" class="min-w-0 text-base font-semibold text-foreground">
              {t("vaultHandoff.qrDialogTitle")}
            </h2>
            <button
              type="button"
              class="flex size-8 shrink-0 items-center justify-center rounded-md text-xl leading-none text-muted-foreground hover:bg-accent hover:text-foreground"
              aria-label={t("common.close")}
              onclick={() => { qrDialogVisible = false; }}
            >
              &times;
            </button>
          </div>
          <p class="mt-1 text-[0.866667rem] leading-5 text-muted-foreground">
            {t("vaultHandoff.qrInstructions")}
          </p>
        </div>
        <div class="mt-5 grid justify-items-center gap-3" role="status" aria-label={t("common.loading")}>
          <div class="grid aspect-square w-full max-w-80 place-items-center bg-white">
            <LoaderCircle size={28} strokeWidth={1.8} class="animate-spin text-black/45" aria-hidden="true" />
          </div>
          <div class="h-5" aria-hidden="true"></div>
        </div>
        <div class="mx-auto mt-3 min-h-9"></div>
      </div>
    </div>
  {:then module}
    {@const PairingQrDialog = module.default}
    <PairingQrDialog
      {invitation}
      invitationError={busy === "invite" ? null : error}
      pendingEnrollment={status?.pendingEnrollment ?? null}
      approvalBusy={busy === "approval"}
      {approvalError}
      onRefresh={() => void showInvitation()}
      onRetry={() => showInvitation()}
      onRespondEnrollment={(approve) => void respondToPendingEnrollment(approve)}
      onClose={closeInvitationDialog}
    />
  {/await}
{/if}

{#if desktopQrAvailable && platform === "desktop" && codeDialogVisible}
  {#await import("./ConnectDialog.svelte") then module}
    {@const ConnectDialog = module.default}
    <ConnectDialog
      busy={busy === "pair"}
      {enrollmentCode}
      error={codeError}
      startOnEmptyServer={presentation === "onboarding"}
      onSubmit={(code) => void acceptInvitation(code)}
      onEdit={() => { codeError = null; }}
      onClose={() => { if (busy !== "pair") { codeDialogVisible = false; codeError = null; } }}
      onServerActivated={() => { codeDialogVisible = false; onActivated?.(); }}
    />
  {/await}
{/if}

{#if confirmation === "network"}
  <ConfirmDialog
    title={t("vaultHandoff.networkAccessConfirmTitle")}
    message={networkAccess?.state === "networkConfirmationRequired"
      ? t("vaultHandoff.networkAccessNetworkConfirmMessage")
      : t("vaultHandoff.networkAccessConfirmMessage")}
    confirmLabel={t("vaultHandoff.allowNetworkAccess")}
    cancelLabel={t("common.cancel")}
    danger={false}
    onConfirm={() => void grantNetworkAccess()}
    onCancel={() => { confirmation = null; }}
  />
{:else if confirmation === "networkRevoke"}
  <ConfirmDialog
    title={t("vaultHandoff.networkAccessRevokeTitle")}
    message={t("vaultHandoff.networkAccessRevokeMessage")}
    confirmLabel={t("vaultHandoff.networkAccessRevokeAction")}
    cancelLabel={t("common.cancel")}
    danger={false}
    onConfirm={() => void revokeNetworkAccess()}
    onCancel={() => { confirmation = null; }}
  />
{:else if confirmation === "unlink"}
  <ConfirmDialog
    title={t("vaultHandoff.unlinkTitle")}
    message={t("vaultHandoff.unlinkMessage")}
    confirmLabel={t("vaultHandoff.unlink")}
    cancelLabel={t("common.cancel")}
    onConfirm={() => void unlink()}
    onCancel={() => { confirmation = null; unlinkDeviceId = null; }}
  />
{:else if confirmation === "recover"}
  <ConfirmDialog
    title={t("vaultHandoff.recoverTitle")}
    message={t("vaultHandoff.recoverMessage")}
    confirmLabel={t("vaultHandoff.recover")}
    cancelLabel={t("common.cancel")}
    onConfirm={() => void recover()}
    onCancel={() => { confirmation = null; }}
  />
{:else if confirmation === "firstCopy"}
  <ConfirmDialog
    title={t("vaultHandoff.firstCopyTitle")}
    message={platform === "android"
      ? t("vaultHandoff.firstCopyAndroid")
      : t("vaultHandoff.firstCopyDesktop")}
    confirmLabel={t("vaultHandoff.firstCopyConfirm")}
    cancelLabel={t("common.cancel")}
    danger={false}
    onConfirm={() => void copyFirst()}
    onCancel={() => { confirmation = null; }}
  />
{/if}
