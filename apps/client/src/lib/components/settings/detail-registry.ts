import {
  createLazyComponentLoader,
  type LazyComponentImporter,
} from "$lib/lazy-component-loader";
import type { SettingsDetailKind } from "$lib/settings/types";

export type LoadedSettingsDetail =
  | {
    kind: "distractions-limit";
    component: typeof import("$lib/components/settings/distractions/DistractionsLimitEditor.svelte").default;
  }
  | {
    kind: "notes-transfer";
    component: typeof import("$lib/components/settings/sections/NotesTransferSettingsPanel.svelte").default;
  }
  | {
    kind: "chat-provider";
    component: typeof import("$lib/components/settings/chat/ProviderSetupPanel.svelte").default;
  };

const DETAIL_IMPORTERS = {
  "distractions-limit": () => import("$lib/components/settings/distractions/DistractionsLimitEditor.svelte")
    .then((module) => ({
      default: { kind: "distractions-limit" as const, component: module.default },
    })),
  "notes-transfer": () => import("$lib/components/settings/sections/NotesTransferSettingsPanel.svelte")
    .then((module) => ({
      default: { kind: "notes-transfer" as const, component: module.default },
    })),
  "chat-provider": () => import("$lib/components/settings/chat/ProviderSetupPanel.svelte")
    .then((module) => ({
      default: { kind: "chat-provider" as const, component: module.default },
    })),
} satisfies Readonly<Record<SettingsDetailKind, LazyComponentImporter<LoadedSettingsDetail>>>;

const loader = createLazyComponentLoader<SettingsDetailKind, LoadedSettingsDetail>(
  DETAIL_IMPORTERS,
);

/** Loads and caches a Settings detail-panel constructor. */
export function loadSettingsDetail(kind: SettingsDetailKind): Promise<LoadedSettingsDetail> {
  return loader.load(kind);
}

/** Retries a failed Settings detail-panel import. */
export function retrySettingsDetail(kind: SettingsDetailKind): Promise<LoadedSettingsDetail> {
  return loader.retry(kind);
}

/** Reports whether a detail-panel constructor is already cached. */
export function settingsDetailHasLoaded(kind: SettingsDetailKind): boolean {
  return loader.hasLoaded(kind);
}
