import type { SettingsDetailKind } from "$lib/settings/types";
import type { LoadedSettingsDetail } from "$lib/components/settings/detail-registry";

function unavailableDetail(kind: SettingsDetailKind): Promise<LoadedSettingsDetail> {
  return Promise.reject(new Error(`Settings detail ${kind} is unavailable on mobile.`));
}

let distractionsLimitLoaded = false;

/** Load the Distractions limit editor and reject desktop-only detail surfaces without importing their component graphs. */
export function loadSettingsDetail(kind: SettingsDetailKind): Promise<LoadedSettingsDetail> {
  if (kind === "distractions-limit") {
    return import("$lib/components/settings/distractions/DistractionsLimitEditor.svelte").then((module) => ({
      kind: "distractions-limit" as const,
      component: module.default,
    })).then((loaded) => {
      distractionsLimitLoaded = true;
      return loaded;
    });
  }
  return unavailableDetail(kind);
}

/** Retry a detail import; desktop-only surfaces reject again. */
export function retrySettingsDetail(kind: SettingsDetailKind): Promise<LoadedSettingsDetail> {
  return loadSettingsDetail(kind);
}

/** Report whether the Distractions limit editor has loaded; desktop-only surfaces never load on mobile. */
export function settingsDetailHasLoaded(kind: SettingsDetailKind): boolean {
  return kind === "distractions-limit" && distractionsLimitLoaded;
}
