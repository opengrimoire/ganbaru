import type { Component } from "svelte";
import Palette from "@lucide/svelte/icons/palette";
import UserRound from "@lucide/svelte/icons/user-round";
import UsersRound from "@lucide/svelte/icons/users-round";
import Calendar from "@lucide/svelte/icons/calendar";
import Folder from "@lucide/svelte/icons/folder";
import Book from "@lucide/svelte/icons/book";
import MessageSquare from "@lucide/svelte/icons/message-square";
import GlobeOff from "@lucide/svelte/icons/globe-off";
import Info from "@lucide/svelte/icons/info";
import Keyboard from "@lucide/svelte/icons/keyboard";
import Music from "@lucide/svelte/icons/music";
import Timer from "@lucide/svelte/icons/timer";
import DownloadCloud from "@lucide/svelte/icons/download-cloud";
import HardDrive from "@lucide/svelte/icons/hard-drive";
import type { SectionId } from "$lib/settings/types";
import type { PlatformShell } from "$lib/platform";

type DataSectionComponent = typeof import("$lib/components/settings/sections/DataSection.svelte").default;

let loadedDataSection: DataSectionComponent | null = null;
let dataSectionLoadRequest: Promise<DataSectionComponent> | null = null;

export interface SettingsSectionMeta {
  id: SectionId;
  labelKey: `settings.section.${SectionId}`;
  icon: Component;
}

export const SETTINGS_SECTIONS: SettingsSectionMeta[] = [
  { id: "appearance", labelKey: "settings.section.appearance", icon: Palette },
  { id: "profile", labelKey: "settings.section.profile", icon: UserRound },
  { id: "contacts", labelKey: "settings.section.contacts", icon: UsersRound },
  { id: "calendars", labelKey: "settings.section.calendars", icon: Calendar },
  { id: "projects", labelKey: "settings.section.projects", icon: Folder },
  { id: "notes", labelKey: "settings.section.notes", icon: Book },
  { id: "chat", labelKey: "settings.section.chat", icon: MessageSquare },
  { id: "focus", labelKey: "settings.section.focus", icon: Timer },
  { id: "music", labelKey: "settings.section.music", icon: Music },
  { id: "distractions", labelKey: "settings.section.distractions", icon: GlobeOff },
  { id: "data", labelKey: "settings.section.data", icon: HardDrive },
  { id: "updates", labelKey: "settings.section.updates", icon: DownloadCloud },
  { id: "shortcuts", labelKey: "settings.section.shortcuts", icon: Keyboard },
  { id: "about", labelKey: "settings.section.about", icon: Info },
];

/** Return the settings categories that have meaning in the selected shell. */
export function settingsSectionsForShell(shell: PlatformShell): readonly SettingsSectionMeta[] {
  if (shell === "desktop") return SETTINGS_SECTIONS;
  return SETTINGS_SECTIONS.filter((section) => section.id !== "shortcuts");
}

/** Loads desktop Data settings once and retains its constructor for synchronous reuse. */
export function preloadDataSection(): Promise<DataSectionComponent> {
  dataSectionLoadRequest ??= import("$lib/components/settings/sections/DataSection.svelte")
    .then((module) => {
      loadedDataSection = module.default;
      return loadedDataSection;
    })
    .catch((error: unknown) => {
      dataSectionLoadRequest = null;
      throw error;
    });
  return dataSectionLoadRequest;
}

/** Returns the preloaded desktop Data constructor without creating a promise. */
export function getPreloadedDataSection(): DataSectionComponent | null {
  return loadedDataSection;
}
