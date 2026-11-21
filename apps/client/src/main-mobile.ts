import { Temporal } from "@js-temporal/polyfill";
import { mount, unmount } from "svelte";
import { ensureConfigLoaded, flushConfig, startConfigWindowSync } from "$lib/vault/config";
import { listenSyncApplied } from "$lib/api/sync";
import { startProfileImageFileSync } from "$lib/stores/profile-image-files.svelte";
import {
  formatVaultError,
  getActiveVaultInfo,
  isLostVaultDatabaseError,
  type VaultInfo,
} from "$lib/vault/state";
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
import { installMobileScrollFades } from "$lib/utils/mobile-scroll-fade";
import { hydrateUserThemes } from "$lib/stores/theme.svelte";

(globalThis as unknown as { Temporal: typeof Temporal }).Temporal = Temporal;
applyPlatformProfileToDocument();
installModalKeyboardRouter();
installMobileScrollFades();
/** Starts system bar inset tracking before any screen mounts; the store loads in the chunk the app shell shares. */
const viewportTracking = import("$lib/stores/viewport.svelte").then((viewport) => viewport.initializeViewportTracking());

type MountedRoot = ReturnType<typeof mount>;
type MobileAppComponent = typeof import("./MobileApp.svelte").default;

function safeStorage(): Storage | undefined {
  try {
    return window.localStorage;
  } catch {
    return undefined;
  }
}

function mountMobileApp(MobileApp: MobileAppComponent, target: HTMLElement): MountedRoot {
  return mount(MobileApp, { target });
}

async function applyPreVaultLanguagePreference(): Promise<void> {
  const storage = safeStorage();
  const preference = readPreVaultLanguagePreference(storage);
  if (!preference) return;
  const applied = await getLocalization().setLanguagePreference(preference);
  if (!applied) return;
  await flushConfig();
  clearPreVaultLanguagePreference(storage);
}

/**
 * Mounts the startup error for a vault that could not open. Android keeps its one vault in private storage, so a
 * vault whose database is gone recovers by restoring one of its rollback copies in place; nothing is removed.
 */
async function mountMobileVaultError(error: unknown) {
  const { default: MobileVaultErrorView } = await import(
    "$lib/components/mobile/MobileVaultErrorView.svelte"
  );
  const lostDatabase = isLostVaultDatabaseError(error);
  return mount(MobileVaultErrorView, {
    target: document.getElementById("app")!,
    props: {
      message: lostDatabase
        ? getLocalization().t("dataFolderError.lostDatabaseMobile")
        : formatVaultError(error, "startup"),
      offerRollbacks: lostDatabase,
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
      onReady: (info: VaultInfo) => {
        void import("$lib/vault/welcome-tour")
          .then(({ markWelcomeTourPending }) => markWelcomeTourPending(safeStorage(), info.vaultId))
          .catch((error: unknown) => console.warn("Could not schedule the welcome tour:", error))
          .finally(() => window.location.reload());
      },
    },
  });
}

/**
 * Mounts the onboarding that follows vault setup, the linking screen, the feature tour, and then the access review,
 * whichever of them is still pending, and opens the app in place once it finishes.
 */
async function mountMobileSetupOnboarding(
  vaultId: string,
  pending: { tourPending: boolean; accessPending: boolean; linkingPending: boolean },
) {
  const target = document.getElementById("app")!;
  const mobileAppModulePromise = import("./MobileApp.svelte");
  void mobileAppModulePromise.catch(() => undefined);
  const { default: MobileSetupOnboarding } = await import(
    "$lib/components/mobile/MobileSetupOnboarding.svelte"
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
  onboardingView = mount(MobileSetupOnboarding, {
    target,
    props: {
      vaultId,
      ...pending,
      onComplete: openApp,
    },
  });
  return onboardingView;
}

const appPromise = (async () => {
  await viewportTracking;
  const preVaultPreference = readPreVaultLanguagePreference(safeStorage());
  await getLocalization().setLanguagePreference(
    preVaultPreference ?? DEFAULT_LANGUAGE_PREFERENCE,
    { persist: false },
  );

  try {
    const activeVault = await getActiveVaultInfo();
    if (!activeVault) return mountMobileVaultSetup(null);
    await ensureConfigLoaded();
    startConfigWindowSync(listenSyncApplied);
    startProfileImageFileSync();
    await initializeLocalizationFromConfig();
    await applyPreVaultLanguagePreference();
    await hydrateUserThemes();
    const { mobileFocusOnboardingCompleted } = await import(
      "$lib/scheduling/mobile-background-execution"
    );
    const { vaultHandoffOnboardingCompleted } = await import(
      "$lib/vault/handoff-onboarding"
    );
    const { welcomeTourPending } = await import("$lib/vault/welcome-tour");
    const tourPending = welcomeTourPending(safeStorage(), activeVault.vaultId);
    const accessPending = !mobileFocusOnboardingCompleted(safeStorage());
    const linkingPending = !vaultHandoffOnboardingCompleted(safeStorage(), activeVault.vaultId);
    if (tourPending || accessPending || linkingPending) {
      return mountMobileSetupOnboarding(activeVault.vaultId, { tourPending, accessPending, linkingPending });
    }
  } catch (error) {
    console.error("Could not load the active vault:", error);
    return mountMobileVaultError(error);
  }

  const { default: MobileApp } = await import("./MobileApp.svelte");
  return mountMobileApp(MobileApp, document.getElementById("app")!);
})();

void appPromise.catch((error: unknown) => {
  console.error("Failed to initialize the mobile application:", error);
});

export default appPromise;
