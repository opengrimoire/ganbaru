import type { LocalRootBinding, MusicItemListEntry } from "$lib/music/library/contracts";
import { resolveLocalMusicPath } from "$lib/music/platform-paths";
import { localFileSourceFromPath, youtubeVideoIdFromIdentity, youtubeVideoSourceFromId } from "$lib/music/sources";
import type { MusicSourceQueueEntry } from "$lib/stores/music-player.svelte";

/** Projects available source-browser tracks into a queue owned by the main player. */
export function projectMusicSourceQueue(
  items: readonly MusicItemListEntry[],
  bindings: readonly LocalRootBinding[],
): MusicSourceQueueEntry[] {
  const bindingPaths = new Map(bindings.map((binding) => [binding.rootId, binding.folderPath]));
  return items.flatMap((item): MusicSourceQueueEntry[] => {
    if (item.sourceKind === "youtube-video") {
      if (item.availability !== "available" && item.availability !== "unknown") return [];
      const videoId = youtubeVideoIdFromIdentity(item.identityKey);
      if (!videoId) return [];
      return [{
        itemId: item.id,
        source: { ...youtubeVideoSourceFromId(videoId), title: item.title },
        snoozed: item.activeSnoozeCount > 0,
      }];
    }
    if (item.availability !== "available") return [];
    if (!item.localRootId || !item.relativePath) return [];
    const rootPath = bindingPaths.get(item.localRootId);
    if (!rootPath) return [];
    const path = resolveLocalMusicPath(rootPath, item.relativePath);
    const sidecarPath = item.originalArtworkIdentity?.startsWith("sidecar:")
      ? resolveLocalMusicPath(rootPath, item.originalArtworkIdentity.slice("sidecar:".length))
      : null;
    return [{
      itemId: item.id,
      source: localFileSourceFromPath(path, item.title, item.artworkOverride ?? sidecarPath),
      snoozed: item.activeSnoozeCount > 0,
    }];
  });
}
