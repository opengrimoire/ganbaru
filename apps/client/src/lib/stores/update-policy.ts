import { translate, type Translate } from "$lib/i18n/translator.svelte";

export const DEFAULT_AUTO_UPDATE_NOTIFICATIONS = true;
export const UPDATE_AUTO_CHECK_INTERVAL_MS = 24 * 60 * 60 * 1000;
const RELEASE_VERSION_PATTERN = /^\d+\.\d+\.\d+(?:[-+][0-9A-Za-z.-]+)?$/u;
const GITHUB_REPOSITORY_PATTERN = /^[A-Za-z0-9_.-]+\/[A-Za-z0-9_.-]+$/u;
const UPDATE_PACKAGE_MANAGERS = new Set([
  "apt",
  "dnf",
  "zypper",
  "aur-yay",
  "aur-paru",
  "fallback",
]);

export type UpdatePackageManager =
  | "apt"
  | "dnf"
  | "zypper"
  | "aur-yay"
  | "aur-paru"
  | "fallback";

export interface UpdateInstallContext {
  selfUpdater: boolean;
  packageManager: UpdatePackageManager | null;
  copyCommand: string | null;
  releasePageUrl: string;
}

export type UpdatePrimaryAction = "self-updater" | "copy-command" | "release-page";
export type AutomaticUpdateCheckKind = "startup" | "periodic";

/**
 * Normalize a stored update check timestamp.
 *
 * @param value Stored config value.
 * @returns ISO timestamp, or null when the value is unusable.
 */
export function parseStoredUpdateCheckAt(value: unknown): string | null {
  if (typeof value !== "string" || !value.trim()) return null;
  const timestamp = Date.parse(value);
  if (!Number.isFinite(timestamp)) return null;
  return new Date(timestamp).toISOString();
}

/**
 * Decide whether the automatic update check can run.
 *
 * @param enabled User preference for automatic update notifications.
 * @param lastCheckAt Last attempted automatic check timestamp.
 * @param nowMs Current wall-clock timestamp.
 * @param kind Startup checks run once per app launch. Periodic checks use the
 * daily timestamp gate for long-running sessions.
 * @returns Whether the automatic check should run now.
 */
export function shouldRunAutomaticUpdateCheck(
  enabled: boolean,
  lastCheckAt: string | null,
  nowMs = Date.now(),
  kind: AutomaticUpdateCheckKind = "periodic",
): boolean {
  if (!enabled) return false;
  if (kind === "startup") return true;
  if (!lastCheckAt) return true;

  const lastCheckMs = Date.parse(lastCheckAt);
  if (!Number.isFinite(lastCheckMs)) return true;
  if (lastCheckMs > nowMs) return true;

  return nowMs - lastCheckMs >= UPDATE_AUTO_CHECK_INTERVAL_MS;
}

/**
 * Build the GitHub release page URL for an updater version.
 *
 * @param repository GitHub repository in owner/name form.
 * @param version Release version reported by the updater.
 * @returns GitHub release page URL, or null when inputs are invalid.
 */
export function releasePageUrl(repository: string, version: string | null): string | null {
  const normalizedRepository = repository.trim();
  const normalizedVersion = version?.trim();
  if (!GITHUB_REPOSITORY_PATTERN.test(normalizedRepository) || !normalizedVersion) {
    return null;
  }
  if (!RELEASE_VERSION_PATTERN.test(normalizedVersion)) return null;

  const tag = `app-v${normalizedVersion}`;
  return `https://github.com/${normalizedRepository}/releases/tag/${encodeURIComponent(tag)}`;
}

/**
 * Build the latest GitHub release page URL for fallback update flows.
 *
 * @param repository GitHub repository in owner/name form.
 * @returns Latest release page URL, or null when the repository is invalid.
 */
export function latestReleasePageUrl(repository: string): string | null {
  const normalizedRepository = repository.trim();
  if (!GITHUB_REPOSITORY_PATTERN.test(normalizedRepository)) return null;
  return `https://github.com/${normalizedRepository}/releases/latest`;
}

/**
 * Parse a backend install-context payload before exposing it to UI state.
 *
 * @param value Unknown IPC payload.
 * @param repository Expected GitHub repository in owner/name form.
 * @returns Safe install context with a release-page fallback.
 */
export function parseUpdateInstallContext(
  value: unknown,
  repository: string,
): UpdateInstallContext {
  const fallback = fallbackUpdateInstallContext(repository);
  if (!isRecord(value)) return fallback;

  const selfUpdater = value.selfUpdater === true;
  const packageManager = parsePackageManager(value.packageManager);
  const copyCommand =
    typeof value.copyCommand === "string" && value.copyCommand.trim().length > 0
      ? value.copyCommand.trim()
      : null;
  const releasePage = parseContextReleasePageUrl(value.releasePageUrl, repository)
    ?? fallback.releasePageUrl;

  return {
    selfUpdater,
    packageManager,
    copyCommand: selfUpdater ? null : copyCommand,
    releasePageUrl: releasePage,
  };
}

/**
 * Choose the primary available-update action for the current install context.
 *
 * @param context Detected install context.
 * @returns Primary action kind.
 */
export function updatePrimaryAction(
  context: UpdateInstallContext | null,
): UpdatePrimaryAction {
  if (context?.selfUpdater) return "self-updater";
  if (context?.copyCommand) return "copy-command";
  return "release-page";
}

function fallbackUpdateInstallContext(repository: string): UpdateInstallContext {
  return {
    selfUpdater: false,
    packageManager: "fallback",
    copyCommand: null,
    releasePageUrl: latestReleasePageUrl(repository)
      ?? "https://github.com/opengrimoire/ganbaru-ai/releases/latest",
  };
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null;
}

function parsePackageManager(value: unknown): UpdatePackageManager | null {
  if (typeof value !== "string") return null;
  return UPDATE_PACKAGE_MANAGERS.has(value) ? (value as UpdatePackageManager) : null;
}

function parseContextReleasePageUrl(value: unknown, repository: string): string | null {
  if (typeof value !== "string") return null;
  const normalized = value.trim();
  const latestUrl = latestReleasePageUrl(repository);
  if (latestUrl && normalized === latestUrl) return normalized;

  const normalizedRepository = repository.trim();
  if (!GITHUB_REPOSITORY_PATTERN.test(normalizedRepository)) return null;
  const tagPrefix = `https://github.com/${normalizedRepository}/releases/tag/app-v`;
  if (!normalized.startsWith(tagPrefix)) return null;
  const version = normalized.slice(tagPrefix.length);
  return RELEASE_VERSION_PATTERN.test(version) ? normalized : null;
}

/**
 * Convert updater errors into user-facing copy.
 *
 * @param error Unknown thrown value.
 * @returns Short error message.
 */
export function updateCheckErrorMessage(
  error: unknown,
  t: Translate = translate,
): string {
  const message = errorText(error);
  if (/endpoint|pubkey|public key|configured|configuration/i.test(message)) {
    return t("updates.feedNotConfigured");
  }
  return message;
}

/**
 * Convert an unknown error value to text.
 *
 * @param error Unknown thrown value.
 * @returns Error text.
 */
export function errorText(error: unknown): string {
  if (error instanceof Error) return error.message;
  if (typeof error === "string") return error;
  return String(error);
}
