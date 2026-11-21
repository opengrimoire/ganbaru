/**
 * What this device knows about a provider CLI, which picks the status and steps the install
 * guide shows.
 */
export type ProviderInstallSituation =
  /** The CLI was found but no configuration uses it yet. */
  | { kind: "installed"; path: string }
  /** The device searched for the CLI and found nothing. */
  | { kind: "not-installed" }
  /** A configuration names a program that is no longer there. */
  | { kind: "missing-at"; path: string }
  /** The CLI runs but its provider account is signed out. */
  | { kind: "signed-out" }
  /** The device does not search for this family's CLI. */
  | { kind: "unknown" };

/**
 * The situation of a family that has no configuration, from the programs the settings read
 * found for it: `undefined` when the device does not search for the family.
 */
export function unconfiguredFamilySituation(found: readonly string[] | undefined): ProviderInstallSituation {
  if (found === undefined) return { kind: "unknown" };
  const first = found[0];
  return first === undefined ? { kind: "not-installed" } : { kind: "installed", path: first };
}

/** Whether the steps start by installing the CLI, rather than only signing in. */
export function situationNeedsInstall(situation: ProviderInstallSituation): boolean {
  return situation.kind === "not-installed" || situation.kind === "missing-at" || situation.kind === "unknown";
}

/** A documentation link as it reads in text: without the scheme or a trailing slash. */
export function installDocsLabel(url: string): string {
  return url.replace(/^https:\/\//, "").replace(/\/$/, "");
}
