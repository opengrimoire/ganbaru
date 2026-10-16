import { Temporal } from "@js-temporal/polyfill";
import { mount, unmount } from "svelte";
import { ensureConfigLoaded, flushConfig, startConfigWindowSync } from "$lib/vault/config";
import { getActiveVaultInfo } from "$lib/vault/state";
import {
  getLocalization,
  initializeLocalizationFromConfig,
} from "$lib/i18n/translator.svelte";
import { DEFAULT_LANGUAGE_PREFERENCE } from "$lib/i18n/locales";
import {
  clearPreVaultLanguagePreference,
  readPreVaultLanguagePreference,
} from "$lib/i18n/pre-vault-language";
import { applyPlatformProfileToDocument } from "$lib/platform";
import { installModalKeyboardRouter } from "$lib/modal-focus";
import { hydrateUserThemes } from "$lib/stores/theme.svelte";
import type { VaultOwnershipTransitionSnapshot } from "$lib/vault/ownership-prompt";

(globalThis as unknown as { Temporal: typeof Temporal }).Temporal = Temporal;
applyPlatformProfileToDocument();
installModalKeyboardRouter();

type MountedRoot = ReturnType<typeof mount>;
type MobileAppComponent = typeof import("./MobileApp.svelte").default;

const MOBILE_OWNERSHIP_RELOAD_EVENT = "ganbaru-ai:mobile-ownership-reload-requested";
const ANDROID_TRANSITION_FRAME_HELD_EVENT = "ganbaru:android-transition-frame-held";
let mobileInitialSurfacePresented: Promise<void> = Promise.resolve();
let ownershipReloadStarted = false;

function safeStorage(): Storage | undefined {
  try {
    return window.localStorage;
  } catch {
    return undefined;
  }
}

async function finishVaultOwnershipTransition(): Promise<void> {
  const cover = document.getElementById("vault-ownership-transition-cover");
  const storageKey = cover?.dataset.storageKey;
  if (!cover || cover.hidden) {
    window.GanbaruAndroidTransition?.releaseHeldFrame();
    return;
  }
  await mobileInitialSurfacePresented;
  await document.fonts.ready;
  await new Promise<void>((resolve) => requestAnimationFrame(() => resolve()));
  await new Promise<void>((resolve) => requestAnimationFrame(() => resolve()));
  cover.hidden = true;
  if (storageKey) {
    try {
      window.sessionStorage.removeItem(storageKey);
    } catch {
      // The cover can still be removed when session storage is unavailable.
    }
  }
  window.GanbaruAndroidTransition?.releaseHeldFrame();
}

function mountMobileApp(MobileApp: MobileAppComponent, target: HTMLElement): MountedRoot {
  let markPresented = (): void => undefined;
  mobileInitialSurfacePresented = new Promise<void>((resolve) => {
    markPresented = resolve;
  });
  return mount(MobileApp, {
    target,
    props: { onInitialSurfacePresented: markPresented },
  });
}

async function reloadAfterMobileOwnershipChange(): Promise<void> {
  try {
    const [
      { readPairingStatus },
      { beginVaultOwnershipTransition, VAULT_OWNERSHIP_TRANSITION_STORAGE_KEY },
    ] = await Promise.all([
      import("$lib/api/vault-handoff"),
      import("$lib/vault/ownership-prompt"),
    ]);
    let snapshotAlreadyPrepared = false;
    try {
      snapshotAlreadyPrepared = window.sessionStorage.getItem(
        VAULT_OWNERSHIP_TRANSITION_STORAGE_KEY,
      ) !== null;
    } catch {
      // Session storage is an optional presentation aid.
    }
    if (snapshotAlreadyPrepared) return;
    const status = await readPairingStatus();
    const { t } = getLocalization();
    const ownerLabel = status.peerLabel ?? t("vaultHandoff.desktopDevice");
    const snapshot = {
      title: t("vaultOwnershipPrompt.title", ownerLabel),
      description: t("vaultOwnershipPrompt.description"),
      actionLabel: t("vaultHandoff.useHere"),
      secondaryLabel: t("vaultOwnershipPrompt.continueReadOnly"),
    } satisfies VaultOwnershipTransitionSnapshot;
    beginVaultOwnershipTransition(snapshot);
    presentVaultOwnershipTransition(snapshot);
    await new Promise<void>((resolve) => requestAnimationFrame(() => resolve()));
    await new Promise<void>((resolve) => requestAnimationFrame(() => resolve()));
  } catch (error) {
    console.warn("Failed to prepare the mobile ownership transition:", error);
  } finally {
    await holdAndroidTransitionFrame();
    window.location.reload();
  }
}

function presentVaultOwnershipTransition(snapshot: VaultOwnershipTransitionSnapshot): void {
  const cover = document.getElementById("vault-ownership-transition-cover");
  const title = document.getElementById("vault-ownership-transition-title");
  const description = document.getElementById("vault-ownership-transition-description");
  const action = document.getElementById("vault-ownership-transition-action");
  const secondary = document.getElementById("vault-ownership-transition-secondary");
  if (!cover || !title || !description || !action || !secondary) return;
  title.textContent = snapshot.title;
  description.textContent = snapshot.description;
  action.textContent = snapshot.actionLabel;
  secondary.textContent = snapshot.secondaryLabel;
  cover.hidden = false;
}

async function holdAndroidTransitionFrame(): Promise<void> {
  const bridge = window.GanbaruAndroidTransition;
  if (!bridge) return;
  await new Promise<void>((resolve) => {
    let settled = false;
    let timeout: number | undefined;
    const finish = (): void => {
      if (settled) return;
      settled = true;
      if (timeout !== undefined) window.clearTimeout(timeout);
      window.removeEventListener(ANDROID_TRANSITION_FRAME_HELD_EVENT, finish);
      resolve();
    };
    timeout = window.setTimeout(finish, 750);
    window.addEventListener(ANDROID_TRANSITION_FRAME_HELD_EVENT, finish, { once: true });
    bridge.holdCurrentFrame();
  });
}

window.addEventListener(MOBILE_OWNERSHIP_RELOAD_EVENT, (event) => {
  event.preventDefault();
  if (ownershipReloadStarted) return;
  ownershipReloadStarted = true;
  void reloadAfterMobileOwnershipChange();
});

async function applyPreVaultLanguagePreference(): Promise<void> {
  const storage = safeStorage();
  const preference = readPreVaultLanguagePreference(storage);
  if (!preference) return;
  const applied = await getLocalization().setLanguagePreference(preference);
  if (!applied) return;
  await flushConfig();
  clearPreVaultLanguagePreference(storage);
}

async function mountMobileVaultError(message: string) {
  const { default: MobileVaultErrorView } = await import(
    "$lib/components/mobile/MobileVaultErrorView.svelte"
  );
  return mount(MobileVaultErrorView, {
    target: document.getElementById("app")!,
    props: {
      message,
      onRetry: () => window.location.reload(),
    },
  });
}

async function mountMobileVaultSetup(initialError: string | null) {
  const { default: MobileVaultSetupView } = await import(
    "$lib/components/mobile/MobileVaultSetupView.svelte"
  );
  return mount(MobileVaultSetupView, {
    target: document.getElementById("app")!,
    props: {
      initialError,
      onReady: () => window.location.reload(),
    },
  });
}

async function mountMobileFocusOnboarding() {
  const { default: MobileFocusOnboarding } = await import(
    "$lib/components/mobile/MobileFocusOnboarding.svelte"
  );
  return mount(MobileFocusOnboarding, {
    target: document.getElementById("app")!,
    props: {
      onComplete: () => window.location.reload(),
    },
  });
}

async function mountMobileVaultHandoffOnboarding(vaultId: string) {
  const target = document.getElementById("app")!;
  const mobileAppModulePromise = import("./MobileApp.svelte");
  void mobileAppModulePromise.catch(() => undefined);
  const { default: MobileVaultHandoffOnboarding } = await import(
    "$lib/components/mobile/MobileVaultHandoffOnboarding.svelte"
  );
  let onboardingView: MountedRoot;
  let transitionPromise: Promise<void> | null = null;
  const openApp = (): Promise<void> => {
    if (transitionPromise) return transitionPromise;
    transitionPromise = mobileAppModulePromise
      .then(async ({ default: MobileApp }) => {
        await unmount(onboardingView);
        mountMobileApp(MobileApp, target);
      })
      .catch((error: unknown) => {
        transitionPromise = null;
        throw error;
      });
    return transitionPromise;
  };
  onboardingView = mount(MobileVaultHandoffOnboarding, {
    target,
    props: {
      vaultId,
      bootstrapReplicaOnLink: true,
      onComplete: openApp,
    },
  });
  return onboardingView;
}

const appPromise = (async () => {
  const preVaultPreference = readPreVaultLanguagePreference(safeStorage());
  await getLocalization().setLanguagePreference(
    preVaultPreference ?? DEFAULT_LANGUAGE_PREFERENCE,
    { persist: false },
  );

  try {
    const activeVault = await getActiveVaultInfo();
    if (!activeVault) return mountMobileVaultSetup(null);
    await ensureConfigLoaded();
    startConfigWindowSync();
    await initializeLocalizationFromConfig();
    await applyPreVaultLanguagePreference();
    await hydrateUserThemes();
    const { mobileFocusOnboardingCompleted } = await import(
      "$lib/scheduling/mobile-background-execution"
    );
    if (!mobileFocusOnboardingCompleted(safeStorage())) {
      return mountMobileFocusOnboarding();
    }
    const { vaultHandoffOnboardingCompleted } = await import(
      "$lib/vault/handoff-onboarding"
    );
    if (!vaultHandoffOnboardingCompleted(safeStorage(), activeVault.vaultId)) {
      return mountMobileVaultHandoffOnboarding(activeVault.vaultId);
    }
  } catch (error) {
    const message = error instanceof Error ? error.message : String(error);
    return mountMobileVaultError(message);
  }

  const { default: MobileApp } = await import("./MobileApp.svelte");
  return mountMobileApp(MobileApp, document.getElementById("app")!);
})();

void appPromise
  .then(() => finishVaultOwnershipTransition())
  .catch(async (error: unknown) => {
    await finishVaultOwnershipTransition();
    console.error("Failed to initialize the mobile application:", error);
  });

export default appPromise;
