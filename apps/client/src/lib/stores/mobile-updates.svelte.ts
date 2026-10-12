import { BUILD_REF, GITHUB_REPOSITORY } from "$lib/build-info";
import { getLocalization } from "$lib/i18n/translator.svelte";
import { errorText, latestReleasePageUrl } from "./update-policy";

const GITHUB_REPOSITORY_PATTERN = /^[A-Za-z0-9_.-]+\/[A-Za-z0-9_.-]+$/u;
const SEMVER_PATTERN =
  /^(?<major>\d+)\.(?<minor>\d+)\.(?<patch>\d+)(?:-(?<prerelease>[0-9A-Za-z.-]+))?(?:\+(?<build>[0-9A-Za-z.-]+))?$/u;
const RELEASE_TAG_PREFIX = "app-v";

export interface ParsedSemVer {
  readonly major: number;
  readonly minor: number;
  readonly patch: number;
  readonly prerelease: readonly string[];
}

export interface LatestRelease {
  readonly version: string;
  readonly releasePageUrl: string;
  readonly apkDownloadUrl: string | null;
  readonly publishedAt: string | null;
}

type RawReleasePayload = Record<string, unknown>;

export type MobileUpdateStatus = "idle" | "checking" | "current" | "available" | "error";

interface CheckOptions {
  force?: boolean;
}

function isRecord(value: unknown): value is RawReleasePayload {
  return typeof value === "object" && value !== null;
}

function isNonBlankString(value: unknown): value is string {
  return typeof value === "string" && value.trim().length > 0;
}

export function parseVersion(version: string): ParsedSemVer | null {
  const normalized = version.trim();
  const match = SEMVER_PATTERN.exec(normalized);
  if (!match?.groups) return null;

  const major = Number.parseInt(match.groups.major, 10);
  const minor = Number.parseInt(match.groups.minor, 10);
  const patch = Number.parseInt(match.groups.patch, 10);
  if (!Number.isFinite(major) || !Number.isFinite(minor) || !Number.isFinite(patch)) return null;

  const prerelease = match.groups.prerelease
    ?.trim()
    .split(".")
    .map((value) => value.trim())
    .filter((value) => value.length > 0) ?? [];

  return {
    major,
    minor,
    patch,
    prerelease,
  };
}

function comparePrerelease(left: readonly string[], right: readonly string[]): number {
  const sharedCount = Math.min(left.length, right.length);

  for (let index = 0; index < sharedCount; index += 1) {
    const leftPart = left[index];
    const rightPart = right[index];
    if (leftPart === rightPart) continue;

    const leftIsNumeric = /^\d+$/u.test(leftPart);
    const rightIsNumeric = /^\d+$/u.test(rightPart);
    if (leftIsNumeric && rightIsNumeric) {
      const leftNumber = Number.parseInt(leftPart, 10);
      const rightNumber = Number.parseInt(rightPart, 10);
      if (Number.isFinite(leftNumber) && Number.isFinite(rightNumber)) {
        if (leftNumber > rightNumber) return 1;
        return -1;
      }
    }

    if (leftIsNumeric) return -1;
    if (rightIsNumeric) return 1;

    return leftPart > rightPart ? 1 : -1;
  }

  if (left.length === right.length) return 0;
  return left.length > right.length ? 1 : -1;
}

export function compareVersions(leftVersion: string, rightVersion: string): number {
  const left = parseVersion(leftVersion);
  const right = parseVersion(rightVersion);
  if (!left || !right) return 0;

  if (left.major !== right.major) return left.major > right.major ? 1 : -1;
  if (left.minor !== right.minor) return left.minor > right.minor ? 1 : -1;
  if (left.patch !== right.patch) return left.patch > right.patch ? 1 : -1;

  const leftPrerelease = left.prerelease;
  const rightPrerelease = right.prerelease;

  if (leftPrerelease.length === 0 && rightPrerelease.length === 0) return 0;
  if (leftPrerelease.length === 0) return 1;
  if (rightPrerelease.length === 0) return -1;

  return comparePrerelease(leftPrerelease, rightPrerelease);
}

export function parseInstalledVersion(buildRef: string): string | null {
  const [version] = buildRef.trim().split("+", 2);
  if (!version) return null;
  return parseVersion(version) ? version : null;
}

export function parseReleaseTag(tagName: string): string | null {
  const normalizedTag = tagName.trim();
  if (!normalizedTag.startsWith(RELEASE_TAG_PREFIX)) return null;
  const version = normalizedTag.slice(RELEASE_TAG_PREFIX.length);
  return parseVersion(version) ? version : null;
}

function parseApkDownloadUrl(
  payload: RawReleasePayload,
  repository: string,
  tagName: string,
  version: string,
): string | null {
  if (!Array.isArray(payload.assets)) return null;

  const expectedName = `Ganbaru_AI_${version}_android_universal.apk`;
  const expectedUrl =
    `https://github.com/${repository}/releases/download/${encodeURIComponent(tagName)}/${encodeURIComponent(expectedName)}`;

  for (const asset of payload.assets) {
    if (!isRecord(asset) || asset.name !== expectedName) continue;
    return asset.browser_download_url === expectedUrl ? expectedUrl : null;
  }

  return null;
}

/**
 * Validate the latest-release response used by the Android updater.
 *
 * @param payload Unknown GitHub API response.
 * @param repository Expected GitHub repository in owner/name form.
 * @returns Validated release details, or null for an unusable response.
 */
export function parseReleasePayload(
  payload: unknown,
  repository = GITHUB_REPOSITORY,
): LatestRelease | null {
  if (!isRecord(payload)) return null;
  if (!GITHUB_REPOSITORY_PATTERN.test(repository)) return null;

  const rawTag = payload.tag_name;
  if (!isNonBlankString(rawTag)) return null;
  const version = parseReleaseTag(rawTag);
  if (!version) return null;

  const publishedAt = isNonBlankString(payload.published_at) ? payload.published_at : null;
  const expectedReleaseUrl =
    `https://github.com/${repository}/releases/tag/${encodeURIComponent(rawTag)}`;
  const htmlUrl = payload.html_url === expectedReleaseUrl ? expectedReleaseUrl : null;
  const fallbackUrl = latestReleasePageUrl(repository);

  const releasePageUrl = htmlUrl ?? fallbackUrl;
  if (!releasePageUrl) return null;

  return {
    version,
    releasePageUrl,
    apkDownloadUrl: parseApkDownloadUrl(payload, repository, rawTag, version),
    publishedAt,
  };
}

function latestReleaseApiUrl(repository: string): string | null {
  if (!GITHUB_REPOSITORY_PATTERN.test(repository)) return null;
  const [owner, name] = repository.split("/", 2);
  if (!owner || !name) return null;

  const encodedOwner = encodeURIComponent(owner);
  const encodedName = encodeURIComponent(name);
  return `https://api.github.com/repos/${encodedOwner}/${encodedName}/releases/latest`;
}

class MobileUpdateStore {
  private localization = getLocalization();
  private checkPromise: Promise<void> | null = null;
  private repository = GITHUB_REPOSITORY;

  status = $state<MobileUpdateStatus>("idle");
  installedVersion = $state<string | null>(parseInstalledVersion(BUILD_REF));
  latestVersion = $state<string | null>(null);
  latestReleaseUrl = $state<string | null>(null);
  latestApkUrl = $state<string | null>(null);
  publishedAt = $state<string | null>(null);
  errorMessage = $state<string | null>(null);

  statusCopy = $derived.by(() => {
    const { t } = this.localization;
    switch (this.status) {
      case "checking":
        return t("updates.checkingFeed");
      case "available":
        return this.latestVersion
          ? t("updates.versionAvailable", this.latestVersion)
          : t("updates.updateAvailable");
      case "current":
        return t("updates.current");
      case "error":
        return this.errorMessage ?? t("updates.checkFailed");
      default:
        return t("updates.notChecked");
    }
  });

  async checkForUpdates(options: CheckOptions = {}): Promise<void> {
    const force = options.force === true;
    if (this.checkPromise) return this.checkPromise;
    if (!force && this.status !== "idle" && this.status !== "error") {
      return;
    }

    this.checkPromise = this.check().finally(() => {
      this.checkPromise = null;
    });
    await this.checkPromise;
  }

  private async check(): Promise<void> {
    if (!this.installedVersion) {
      this.status = "error";
      this.errorMessage = this.localization.t("updates.feedNotConfigured");
      return;
    }

    const endpoint = latestReleaseApiUrl(this.repository);
    if (!endpoint) {
      this.status = "error";
      this.errorMessage = this.localization.t("updates.feedNotConfigured");
      return;
    }

    this.status = "checking";
    this.errorMessage = null;

    try {
      const response = await fetch(endpoint, {
        headers: {
          Accept: "application/vnd.github+json",
        },
      });

      if (!response.ok) {
        let message = this.localization.t("updates.checkFailed");
        try {
          const body = (await response.json()) as { message?: unknown };
          if (isRecord(body) && isNonBlankString(body.message) && body.message.length > 0) {
            message = body.message;
          }
        } catch {
          message = `${message}: ${response.statusText || response.status}`;
        }
        throw new Error(message);
      }

      const release = parseReleasePayload(await response.json(), this.repository);
      if (!release) throw new Error(this.localization.t("updates.feedNotConfigured"));

      this.latestVersion = release.version;
      this.latestReleaseUrl = release.releasePageUrl;
      this.latestApkUrl = release.apkDownloadUrl;
      this.publishedAt = release.publishedAt;

      if (compareVersions(release.version, this.installedVersion) > 0) {
        this.status = "available";
      } else {
        this.status = "current";
      }
      this.errorMessage = null;
    } catch (error: unknown) {
      this.status = "error";
      this.errorMessage = errorText(error);
    }
  }
}

let store: MobileUpdateStore | null = null;

export function getMobileUpdateManager(): MobileUpdateStore {
  store ??= new MobileUpdateStore();
  return store;
}
