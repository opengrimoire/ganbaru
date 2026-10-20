import type { LocalRootBinding, MusicItemListEntry } from "$lib/music/library/contracts";
import { sidecarArtworkRelativePath } from "$lib/music/artwork-identity";
import { resolveLocalMusicPath } from "$lib/music/platform-paths";

export type MusicListArtworkSource =
  | { kind: "file"; path: string }
  | { kind: "embedded"; path: string; identity: string };

/** Resolves list artwork from the same scanner metadata used by Review. */
export function musicListArtworkSource(
  item: MusicItemListEntry,
  bindings: readonly LocalRootBinding[],
): MusicListArtworkSource | null {
  if (item.artworkOverride) return { kind: "file", path: item.artworkOverride };
  if (item.sourceKind !== "local-file" || !item.localRootId) return null;
  const root = bindings.find((binding) => binding.rootId === item.localRootId)?.folderPath;
  if (!root) return null;
  if (item.originalArtworkIdentity?.startsWith("sidecar:")) {
    const sidecar = sidecarArtworkRelativePath(item.originalArtworkIdentity);
    return sidecar ? { kind: "file", path: resolveLocalMusicPath(root, sidecar) } : null;
  }
  if (item.originalArtworkIdentity?.startsWith("embedded:") && item.relativePath) {
    return {
      kind: "embedded",
      path: resolveLocalMusicPath(root, item.relativePath),
      identity: item.originalArtworkIdentity,
    };
  }
  return null;
}
