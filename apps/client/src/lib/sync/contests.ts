import type {
  SyncCandidateKind,
  SyncContest,
  SyncContestCandidate,
  SyncContestObjection,
  SyncContests,
  SyncDeviceRef,
  SyncKitPoint,
  SyncRecordStorage,
} from "$lib/api/sync";
import { formatNumber } from "$lib/i18n/formatters";
import type { AppLocale } from "$lib/i18n/locales";
import type { Translate } from "$lib/i18n/translator.svelte";
import { syncDeviceName } from "./status";

const BYTES_PER_MEGABYTE = 1_000_000;

/** Why the kit's device review lists a device. */
export type SyncKitReviewReason = "flooder" | "losingRemoval";

/** A device the recovery kit's device review lists. */
export interface SyncKitReviewDevice {
  device: SyncDeviceRef;
  reason: SyncKitReviewReason;
  /** For a losing removal, the device that removed it, null for a recovery key. */
  removedBy: SyncDeviceRef | null;
  /** Whether the review marks it before the user changes anything. */
  preselected: boolean;
}

/** What one candidate of a contest offers this device. */
export interface SyncCandidateActions {
  yieldTo: boolean;
  object: boolean;
  withdrawObjection: boolean;
}

/** Names a device in contest and storage lines. */
export function syncContestDeviceName(t: Translate, device: SyncDeviceRef): string {
  return syncDeviceName(device, t("sync.authority.thisDevice"), t("sync.authority.unnamedDevice"));
}

/**
 * The devices the recovery kit's device review lists. Only a device the storage alerts name as
 * flooding the record store or as forking its writer starts marked; a device that a losing
 * branch removed is listed unmarked with who removed it, since that removal may be the
 * attacker's. This device is never listed, and a device appears once, marked if any reason
 * marks it.
 */
export function syncKitReviewDevices(contests: SyncContests | null): SyncKitReviewDevice[] {
  if (!contests) return [];
  const review = new Map<string, SyncKitReviewDevice>();
  for (const alert of contests.storage.alerts) {
    const device = alert.kind === "total" ? null : alert.device;
    if (!device || device.ownDevice || review.has(device.deviceId)) continue;
    review.set(device.deviceId, { device, reason: "flooder", removedBy: null, preselected: true });
  }
  for (const removal of contests.losingRemovals) {
    if (removal.device.ownDevice || review.has(removal.device.deviceId)) continue;
    review.set(removal.device.deviceId, {
      device: removal.device,
      reason: "losingRemoval",
      removedBy: removal.removedBy,
      preselected: false,
    });
  }
  return [...review.values()];
}

/** The kit points this desktop may continue from, the default first, or none without a contest. */
export function syncKitPoints(contests: SyncContests | null): SyncKitPoint[] {
  for (const contest of contests?.contests ?? []) {
    for (const action of contest.actions) {
      if (action.kind === "useKit") return action.points;
    }
  }
  return [];
}

/** Whether a contest offers this desktop the recovery kit. */
export function syncKitContested(contests: SyncContests | null): boolean {
  return syncKitPoints(contests).length > 0;
}

/** Labels a kit point by where it lies. */
export function syncKitPointLabel(t: Translate, point: SyncKitPoint): string {
  switch (point.place.kind) {
    case "head": return t("sync.kit.pointHead");
    case "candidate": return t("sync.kit.pointCandidate", syncContestDeviceName(t, point.place.target));
    case "continuation": return t("sync.kit.pointContinuation", syncContestDeviceName(t, point.place.anchor));
  }
}

/** The yields and objections a contest offers this device on `candidate`. */
export function syncCandidateActions(contest: SyncContest, candidate: SyncContestCandidate): SyncCandidateActions {
  const offered = (kind: "yieldTo" | "object" | "withdrawObjection") =>
    contest.actions.some((action) => action.kind === kind && action.candidate === candidate.hash);
  return { yieldTo: offered("yieldTo"), object: offered("object"), withdrawObjection: offered("withdrawObjection") };
}

/** The objection that still stands against `candidate`, if any. */
export function syncLiveObjection(contest: SyncContest, candidate: SyncContestCandidate): SyncContestObjection | null {
  return contest.objections.find((objection) => objection.against === candidate.hash && !objection.withdrawn) ?? null;
}

/** Explains why a contest stopped device changes. */
export function syncContestCauseLine(t: Translate, contest: SyncContest): string {
  switch (contest.cause) {
    case "contested": return t("sync.contest.cause.contested");
    case "writerFork": return t("sync.contest.cause.writerFork");
    case "taken": {
      const taker = contest.candidates.find((candidate) => !candidate.local) ?? contest.candidates[0];
      return t("sync.contest.cause.taken", taker ? syncContestDeviceName(t, taker.target) : t("sync.authority.unnamedDevice"));
    }
  }
}

function candidateKindLabel(t: Translate, kind: SyncCandidateKind): string {
  switch (kind) {
    case "planned": return t("sync.contest.kind.planned");
    case "forced": return t("sync.contest.kind.forced");
    case "selfHandover": return t("sync.contest.kind.selfHandover");
    case "kit": return t("sync.contest.kind.kit");
    case "installedKey": return t("sync.contest.kind.installedKey");
  }
}

/**
 * The details shown under a candidate: how it was signed, when and by whom its target was
 * enrolled, and whether it was withdrawn or blocked.
 */
export function syncCandidateDetails(t: Translate, contest: SyncContest, candidate: SyncContestCandidate): string[] {
  const lines = [candidateKindLabel(t, candidate.kind)];
  if (candidate.targetEnrolledRevision !== null) {
    lines.push(
      candidate.targetEnroller
        ? t("sync.contest.enrolled", candidate.targetEnrolledRevision, syncContestDeviceName(t, candidate.targetEnroller))
        : t("sync.contest.enrolledByKey", candidate.targetEnrolledRevision),
    );
  }
  if (candidate.withdrawnInFavorOf !== null) {
    const favored = contest.candidates.find((entry) => entry.hash === candidate.withdrawnInFavorOf);
    if (favored) lines.push(t("sync.contest.withdrawn", syncContestDeviceName(t, favored.target)));
  }
  const objection = syncLiveObjection(contest, candidate);
  if (objection) {
    lines.push(
      objection.objector
        ? t("sync.contest.blocked", syncContestDeviceName(t, objection.objector))
        : t("sync.contest.blockedByAnchor"),
    );
  }
  return lines;
}

function formatMegabytes(locale: AppLocale, bytes: number): string {
  return formatNumber(locale, bytes / BYTES_PER_MEGABYTE, {
    style: "unit",
    unit: "megabyte",
    maximumFractionDigits: 1,
  });
}

/** The growth alerts of the record store as status lines. */
export function syncStorageAlertLines(t: Translate, locale: AppLocale, storage: SyncRecordStorage): string[] {
  return storage.alerts.map((alert) => {
    switch (alert.kind) {
      case "device":
        return t("sync.storage.alertDevice", syncContestDeviceName(t, alert.device), formatMegabytes(locale, alert.bytes));
      case "total": return t("sync.storage.alertTotal", formatMegabytes(locale, alert.bytes));
      case "forkedWriter":
        return alert.device
          ? t("sync.storage.alertForked", syncContestDeviceName(t, alert.device))
          : t("sync.storage.alertForkedUnknown");
    }
  });
}
