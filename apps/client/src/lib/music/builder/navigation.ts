import {
  MUSIC_BUILDER_PRIMARY_DESTINATIONS,
  musicBuilderDestinationShortcut,
  type MusicBuilderDestination,
  type MusicBuilderPrimaryDestinationKind,
} from "./routing";

export interface MusicBuilderDockItem {
  kind: MusicBuilderPrimaryDestinationKind;
  active: boolean;
  badge: number | null;
  shortcut: string;
}

/** Projects stable navigation dock items from builder state. */
export function projectMusicBuilderDockItems(
  destination: MusicBuilderDestination,
  reviewCount: number,
  includeSoundscapes = true,
): MusicBuilderDockItem[] {
  const activeKind = destination.kind === "playlist" ? "playlists" : destination.kind;
  return MUSIC_BUILDER_PRIMARY_DESTINATIONS
    .filter((kind) => includeSoundscapes || kind !== "soundscapes")
    .map((kind) => ({
      kind,
      active: kind === activeKind,
      badge: kind === "review" ? positiveBadge(reviewCount) : null,
      shortcut: musicBuilderDestinationShortcut(kind) ?? "",
    }));
}

function positiveBadge(value: number): number | null {
  return Number.isFinite(value) && value > 0 ? Math.floor(value) : null;
}
