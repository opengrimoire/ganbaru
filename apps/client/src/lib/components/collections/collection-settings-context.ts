import { getContext, setContext, type Snippet } from "svelte";

/** A detail page inside the single collection settings popover. */
export interface CollectionSettingsPage {
  label: string;
  children: Snippet;
  trigger: HTMLButtonElement;
}

/** Navigation shared by settings rows and their owning floating panel. */
export interface CollectionSettingsNavigation {
  navigate: (page: CollectionSettingsPage) => void;
  isActive: (trigger: HTMLButtonElement | undefined) => boolean;
}

const COLLECTION_SETTINGS_CONTEXT = Symbol("collection-settings-navigation");

/** Register navigation for descendants rendered inside collection settings. */
export function setCollectionSettingsNavigation(navigation: CollectionSettingsNavigation): void {
  setContext(COLLECTION_SETTINGS_CONTEXT, navigation);
}

/** Resolve the nearest settings popover, leaving ordinary menus independent. */
export function getCollectionSettingsNavigation(): CollectionSettingsNavigation | undefined {
  return getContext<CollectionSettingsNavigation | undefined>(COLLECTION_SETTINGS_CONTEXT);
}
