const SIDECAR_IDENTITY_PREFIX = "sidecar:";

/**
 * Returns the root-relative path stored in a sidecar artwork identity, or null when the identity
 * is not a sidecar or its path could leave the source root.
 *
 * Artwork identities come from vault data, so absolute, rooted, and drive-qualified paths and
 * empty, `.`, or `..` segments are refused before the path is joined to a local root.
 */
export function sidecarArtworkRelativePath(identity: string | null | undefined): string | null {
  if (!identity?.startsWith(SIDECAR_IDENTITY_PREFIX)) return null;
  const relativePath = identity.slice(SIDECAR_IDENTITY_PREFIX.length);
  if (/^[A-Za-z]:/u.test(relativePath) || relativePath.includes("\0")) return null;
  const segments = relativePath.split(/[\\/]/u);
  if (segments.some((segment) => segment === "" || segment === "." || segment === "..")) return null;
  return relativePath;
}
