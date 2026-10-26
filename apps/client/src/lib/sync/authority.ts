import type {
  SyncAuthorityDevice,
  SyncAuthorityView,
  SyncLateChange,
  SyncLateKind,
  SyncOutboxEntry,
  SyncOutboxState,
  SyncProposalRejection,
  SyncRole,
  SyncState,
  SyncStatusView,
} from "$lib/api/sync";
import type { AppLocale } from "$lib/i18n/locales";
import { formatDateTime } from "$lib/i18n/formatters";
import type { Translate } from "$lib/i18n/translator.svelte";
import { formatSyncTime, syncDeviceName } from "./status";

/** How long changes may wait for the anchor before the interface suggests taking it over. */
export const SYNC_HANDOVER_SUGGESTION_MS = 30 * 24 * 60 * 60 * 1000;

/** Kit code groups shown on one line of the code display and the saved file. */
export const SYNC_KIT_GROUPS_PER_LINE = 4;

/** Separator between groups in a kit code, as the native side formats it. */
const KIT_GROUP_SEPARATOR = "-";

/** Statuses in which the anchor answered recently enough to sequence a proposal soon. */
const ANCHOR_REACHABLE_STATES: readonly SyncState[] = ["idle", "syncing"];

/** A role or state badge of an enrolled device. */
export type SyncDeviceBadge = "anchor" | "admin" | "member" | "removed" | "quarantined";

/** What the authority controls may do on this device. */
export interface SyncAuthorityContext {
  authority: SyncAuthorityView;
  /** Role of this installation in the LAN topology, from the sync status. */
  statusRole: SyncRole | null;
  /** Whether this is the Android layout, where admin actions are never offered. */
  mobileLayout: boolean;
}

/** The actions offered for one enrolled device. */
export interface SyncDeviceActions {
  remove: boolean;
  quarantine: boolean;
  lift: boolean;
  promote: boolean;
}

/**
 * Badges of an enrolled device: its role, or that it was removed, then whether it is the anchor
 * and whether this installation quarantined it.
 */
export function syncDeviceBadges(device: SyncAuthorityDevice): SyncDeviceBadge[] {
  const badges: SyncDeviceBadge[] = [device.removedRevision === null ? device.role : "removed"];
  if (device.anchor) badges.push("anchor");
  if (device.quarantined) badges.push("quarantined");
  return badges;
}

/** Labels a device badge. */
export function syncDeviceBadgeLabel(t: Translate, badge: SyncDeviceBadge): string {
  switch (badge) {
    case "anchor": return t("sync.authority.role.anchor");
    case "admin": return t("sync.authority.role.admin");
    case "member": return t("sync.authority.role.member");
    case "removed": return t("sync.authority.role.removed");
    case "quarantined": return t("sync.authority.role.quarantined");
  }
}

/** Whether this installation can sign admin proposals here: an admin desktop layout. */
function adminHere(context: SyncAuthorityContext): boolean {
  return !context.mobileLayout && context.authority.ownIsAdmin;
}

/** The enrolled entry of this device, if it is enrolled. */
export function syncOwnDevice(authority: SyncAuthorityView): SyncAuthorityDevice | null {
  return authority.devices.find((entry) => entry.device.ownDevice) ?? null;
}

/**
 * The actions offered for `device`.
 *
 * Quarantine is local to this installation, so any device may use it on another live device
 * that is not the anchor. Removal and promotion are admin proposals, so they need an admin
 * desktop and a space that is not frozen. The coordinator removes devices by unlinking them
 * and is the only installation that promotes, since it holds the admin keys sent at pairing.
 */
export function syncDeviceActions(context: SyncAuthorityContext, device: SyncAuthorityDevice): SyncDeviceActions {
  const other = !device.device.ownDevice;
  const live = device.removedRevision === null;
  const proposing = adminHere(context) && !context.authority.frozen;
  return {
    remove: proposing && context.statusRole !== "hub" && other && live && !device.anchor,
    quarantine: other && live && !device.anchor && !device.quarantined,
    lift: other && device.quarantined,
    promote:
      proposing
      && context.statusRole === "hub"
      && other
      && live
      && !device.quarantined
      && device.kind === "desktop"
      && device.role === "member",
  };
}

/** Whether this linked admin desktop may take the anchor in a planned handover. */
export function canTakeSyncAnchor(context: SyncAuthorityContext): boolean {
  const own = syncOwnDevice(context.authority);
  return adminHere(context)
    && !context.authority.frozen
    && context.statusRole === "client"
    && own !== null
    && own.removedRevision === null
    && !own.anchor;
}

/** Whether this device may keep or discard late changes. */
export function canResolveSyncLateChanges(context: SyncAuthorityContext): boolean {
  return adminHere(context);
}

/** Whether this device may show, save, and replace the recovery kit. */
export function canManageSyncRecoveryKit(context: SyncAuthorityContext): boolean {
  return adminHere(context);
}

/**
 * Whether this desktop may use a recovery kit: an enrolled desktop that is not an admin yet,
 * whose last exchange reached the anchor. Using a kit without a live anchor is not offered.
 */
export function canUseSyncRecoveryKit(context: SyncAuthorityContext, state: SyncState): boolean {
  const own = syncOwnDevice(context.authority);
  return !context.mobileLayout
    && !context.authority.ownIsAdmin
    && !context.authority.frozen
    && own !== null
    && own.kind === "desktop"
    && own.removedRevision === null
    && ANCHOR_REACHABLE_STATES.includes(state);
}

/** Whether this device may reset sync state: only the coordinating desktop can. */
export function canResetSyncState(statusRole: SyncRole | null, mobileLayout: boolean): boolean {
  return !mobileLayout && statusRole === "hub";
}

/** Names an enrolled device for the authority list. */
export function syncAuthorityDeviceName(t: Translate, device: SyncAuthorityDevice): string {
  return syncDeviceName(device.device, t("sync.authority.thisDevice"), t("sync.authority.unnamedDevice"));
}

/**
 * Splits a kit code into display lines of whole groups. The code arrives grouped by the native
 * side; the groups are kept as they are.
 */
export function syncKitCodeLines(code: string, groupsPerLine = SYNC_KIT_GROUPS_PER_LINE): string[] {
  if (!Number.isSafeInteger(groupsPerLine) || groupsPerLine < 1) throw new Error("groupsPerLine must be a positive integer");
  const groups = code.trim().split(KIT_GROUP_SEPARATOR).filter((group) => group.length > 0);
  const lines: string[] = [];
  for (let index = 0; index < groups.length; index += groupsPerLine) {
    lines.push(groups.slice(index, index + groupsPerLine).join(KIT_GROUP_SEPARATOR));
  }
  return lines;
}

/** Default file name of a saved recovery kit. */
export function syncKitFileName(t: Translate): string {
  return `${t("sync.kit.fileName")}.txt`;
}

/**
 * Composes the plain text recovery kit file: title, save time, warning, and the grouped code.
 *
 * @param savedAtMs When the user saves the file, in Unix milliseconds.
 */
export function syncKitFileText(t: Translate, locale: AppLocale, code: string, savedAtMs: number): string {
  return [
    t("sync.kit.fileTitle"),
    "",
    t("sync.kit.fileCreated", formatDateTime(locale, savedAtMs, { dateStyle: "long", timeStyle: "short" })),
    "",
    t("sync.kit.fileWarning"),
    "",
    t("sync.kit.fileCode"),
    ...syncKitCodeLines(code),
    "",
  ].join("\n");
}

/** Labels what a proposal changes, naming its device when it names one. */
export function syncOutboxChangeLabel(t: Translate, entry: SyncOutboxEntry): string {
  const device = entry.target
    ? syncDeviceName(entry.target, t("sync.authority.thisDevice"), t("sync.authority.unnamedDevice"))
    : t("sync.authority.unnamedDevice");
  switch (entry.change) {
    case "enroll": return t("sync.outbox.change.enroll", device);
    case "remove": return t("sync.outbox.change.remove", device);
    case "makeAdmin": return t("sync.outbox.change.makeAdmin", device);
    case "makeMember": return t("sync.outbox.change.makeMember", device);
    case "replaceRecoveryKit": return t("sync.outbox.change.replaceRecoveryKit");
    case "resolveLate": return t("sync.outbox.change.resolveLate");
    case "handover": return t("sync.outbox.change.handover");
    case "unknown": return t("sync.outbox.change.unknown");
  }
}

function outboxStateLabel(t: Translate, state: SyncOutboxState): string {
  switch (state) {
    case "pending": return t("sync.outbox.state.pending");
    case "held": return t("sync.outbox.state.held");
    case "rejected": return t("sync.outbox.state.rejected");
  }
}

/** Explains why the anchor refused a proposal. */
export function syncRejectionLabel(t: Translate, rejection: SyncProposalRejection): string {
  switch (rejection) {
    case "not_anchor": return t("sync.outbox.rejection.not_anchor");
    case "wrong_space": return t("sync.outbox.rejection.wrong_space");
    case "not_admin": return t("sync.outbox.rejection.not_admin");
    case "malformed": return t("sync.outbox.rejection.malformed");
    case "unsupported_entry": return t("sync.outbox.rejection.unsupported_entry");
    case "already_enrolled": return t("sync.outbox.rejection.already_enrolled");
    case "unknown_device": return t("sync.outbox.rejection.unknown_device");
    case "device_removed": return t("sync.outbox.rejection.device_removed");
    case "names_anchor": return t("sync.outbox.rejection.names_anchor");
    case "admin_not_desktop": return t("sync.outbox.rejection.admin_not_desktop");
    case "last_admin": return t("sync.outbox.rejection.last_admin");
    case "unknown_removal": return t("sync.outbox.rejection.unknown_removal");
    case "handover_conflict": return t("sync.outbox.rejection.handover_conflict");
    case "epoch_frozen": return t("sync.outbox.rejection.epoch_frozen");
    case "catching_up": return t("sync.outbox.rejection.catching_up");
  }
}

/** Describes where a proposal stands, with the anchor's reason when it was refused. */
export function syncOutboxStateLine(t: Translate, entry: SyncOutboxEntry): string {
  const state = outboxStateLabel(t, entry.state);
  return entry.rejection ? `${state}. ${syncRejectionLabel(t, entry.rejection)}` : state;
}

function lateKindLabel(t: Translate, kind: SyncLateKind): string {
  switch (kind) {
    case "values": return t("sync.late.kind.values");
    case "tombstone": return t("sync.late.kind.tombstone");
    case "created": return t("sync.late.kind.created");
  }
}

/** Says what the removed device did to the row, on which device, and when when known. */
export function syncLateChangeOrigin(
  t: Translate,
  locale: AppLocale,
  change: SyncLateChange,
  nowMs: number,
): string {
  const kind = lateKindLabel(t, change.kind);
  const device = syncDeviceName(change.device, t("sync.recovery.thisDevice"), t("sync.recovery.otherDevice"));
  if (change.changedAtMs === null) return t("sync.late.by", kind, device);
  return t("sync.late.byAt", kind, device, formatSyncTime(t, locale, change.changedAtMs, nowMs));
}

/** Stable key of a late change in lists and busy state. */
export function syncLateChangeKey(change: SyncLateChange): string {
  return `${change.removalRevision}:${change.table}:${change.rowKey}`;
}

/** What the interface says about changes the anchor has not confirmed storing. */
export interface SyncUnconfirmedNotice {
  lines: string[];
  /** Whether the oldest change waited long enough to suggest taking the anchor. */
  suggestHandover: boolean;
}

/**
 * Describes unconfirmed changes, or returns null when there are none.
 *
 * @param thresholdMs Age of the oldest unconfirmed change after which a handover is suggested.
 */
export function syncUnconfirmedNotice(
  t: Translate,
  locale: AppLocale,
  status: SyncStatusView,
  nowMs: number,
  thresholdMs = SYNC_HANDOVER_SUGGESTION_MS,
): SyncUnconfirmedNotice | null {
  if (status.unconfirmed === 0) return null;
  const lines = [t("sync.unconfirmed", status.unconfirmed)];
  const oldest = status.oldestUnconfirmedAtMs;
  if (oldest !== null) lines.push(t("sync.unconfirmedSince", formatSyncTime(t, locale, oldest, nowMs)));
  const suggestHandover = oldest !== null && nowMs - oldest >= thresholdMs;
  if (suggestHandover) lines.push(t("sync.unconfirmedHandover"));
  return { lines, suggestHandover };
}
