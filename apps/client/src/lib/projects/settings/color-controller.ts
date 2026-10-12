import type { EventColor } from "$lib/calendar/types";
import { nextUnusedProjectSettingsColor } from "./ui";

interface ColorEntry { id: string }

export interface ProjectSettingsColorCollection<T extends ColorEntry> {
  entries: () => readonly T[];
  color: (entry: T) => EventColor;
  fallback: EventColor;
}

/** Allocates the next unused palette color from the current collection draft. */
export function createProjectSettingsColorAllocator<T extends ColorEntry>(
  collection: ProjectSettingsColorCollection<T>,
) {
  return (preferredColor: EventColor, extraColor?: EventColor): EventColor => {
    const usedColors = new Set(collection.entries().map(collection.color));
    if (extraColor !== undefined) usedColors.add(extraColor);
    return nextUnusedProjectSettingsColor({
      preferredColor,
      usedColors,
      fallbackColor: collection.fallback,
    });
  };
}
