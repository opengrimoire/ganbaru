import { Temporal } from "@js-temporal/polyfill";
import { mount, unmount } from "svelte";
import { ensureConfigLoaded, flushConfig, startConfigWindowSync } from "$lib/vault/config";
import { listenSyncApplied } from "$lib/api/sync";
import { startProfileImageFileSync } from "$lib/stores/profile-image-files.svelte";
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

(globalThis as unknown as { Temporal: typeof Temporal }).Temporal = Temporal;
applyPlatformProfileToDocument();
installModalKeyboardRouter();

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
    startConfigWindowSync(listenSyncApplied);
    startProfileImageFileSync();
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

void appPromise.catch((error: unknown) => {
  console.error("Failed to initialize the mobile application:", error);
});

export default appPromise;
