/**
 * Shared vocabulary for the Contacts experience: roles, capabilities, invitation states, sharing options, and the
 * placeholder shown behind the blurred contact card. docs/features/collaboration/README.md owns the product rules.
 */

export const MEMBER_ROLES = ["owner", "administrator", "member", "guest", "custom"] as const;
export type MemberRole = (typeof MEMBER_ROLES)[number];

/** Roles a person can be invited with; ownership is transferred, never granted through an invitation. */
export const INVITABLE_MEMBER_ROLES = ["administrator", "member", "guest", "custom"] as const;

export const MEMBER_CAPABILITY_GROUPS = ["conversation", "content", "management"] as const;
export type MemberCapabilityGroup = (typeof MEMBER_CAPABILITY_GROUPS)[number];

export const MEMBER_CAPABILITIES = [
  { id: "readHistory", group: "conversation" },
  { id: "participate", group: "conversation" },
  { id: "assignWork", group: "conversation" },
  { id: "editNotes", group: "content" },
  { id: "editTasks", group: "content" },
  { id: "editEvents", group: "content" },
  { id: "sharePages", group: "content" },
  { id: "manageChannels", group: "management" },
  { id: "manageMembers", group: "management" },
  { id: "manageTeammates", group: "management" },
  { id: "editSpaceSettings", group: "management" },
] as const satisfies readonly { id: string; group: MemberCapabilityGroup }[];
export type MemberCapability = (typeof MEMBER_CAPABILITIES)[number]["id"];

const ROLE_GROUPS: Record<Exclude<MemberRole, "guest">, readonly MemberCapabilityGroup[]> = {
  owner: MEMBER_CAPABILITY_GROUPS,
  administrator: MEMBER_CAPABILITY_GROUPS,
  member: ["conversation", "content"],
  custom: ["conversation", "content"],
};

const GUEST_CAPABILITIES: readonly MemberCapability[] = ["readHistory", "participate"];

/** Capabilities a role preset grants. Custom starts from the Member preset so the matrix is never empty. */
export function capabilitiesForRole(role: MemberRole): ReadonlySet<MemberCapability> {
  if (role === "guest") return new Set(GUEST_CAPABILITIES);
  const groups = new Set<MemberCapabilityGroup>(ROLE_GROUPS[role]);
  return new Set(MEMBER_CAPABILITIES.filter((capability) => groups.has(capability.group)).map((capability) => capability.id));
}

export const INVITATION_HISTORY_BOUNDARIES = ["entire", "fromAcceptance"] as const;
export type InvitationHistoryBoundary = (typeof INVITATION_HISTORY_BOUNDARIES)[number];

export const INVITATION_STATES = ["pending", "accepted", "declined", "revoked", "expired"] as const;
export type InvitationState = (typeof INVITATION_STATES)[number];

export const INVITATION_SPACE_KINDS = ["group", "project", "channel", "page", "event", "conversation"] as const;
export type InvitationSpaceKind = (typeof INVITATION_SPACE_KINDS)[number];

/** A space an invitation targets, as shown in the invitation dialog. The id preselects it in the space picker. */
export interface InvitationSpaceSummary {
  readonly kind: InvitationSpaceKind;
  readonly name: string;
  readonly id?: string;
}

export const NOTES_SHARE_ROLES = ["view", "comment", "edit", "full"] as const;
export type NotesShareRole = (typeof NOTES_SHARE_ROLES)[number];

export const NOTES_SHARE_SCOPES = ["page", "subtree"] as const;
export type NotesShareScope = (typeof NOTES_SHARE_SCOPES)[number];

export const NOTES_LINK_ACCESS = ["off", "view", "comment"] as const;
export type NotesLinkAccess = (typeof NOTES_LINK_ACCESS)[number];

/**
 * About two thirds of a real code carrying a short display name and a LAN address (roughly 250 characters), so the
 * placeholder reads as the same kind of text without passing for the real code showing through the blur.
 */
export const CONTACT_CARD_PLACEHOLDER_LENGTH = 168;
/** The base64url alphabet real codes use, so the placeholder has the same texture. */
const PLACEHOLDER_CODE_ALPHABET = "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";
/** Seed behind the blurred card, so the hidden QR and code never derive from the real identity. */
const CONTACT_CARD_PLACEHOLDER_SEED = "ganbaru-ai:contact-card-placeholder";
/**
 * Width of a version 6 QR code. A typical real code reaches version 10 (57 modules), so the placeholder is visibly
 * coarser and never looks like the real QR code under the blur.
 */
export const CONTACT_CARD_MATRIX_SIZE = 41;
const FINDER_SIZE = 7;

/** FNV-1a over UTF-16 code units, folded into a 32-bit unsigned integer. */
function fnv1a(text: string, seed = 0x811c9dc5): number {
  let hash = seed >>> 0;
  for (let index = 0; index < text.length; index += 1) {
    hash ^= text.charCodeAt(index);
    hash = Math.imul(hash, 0x01000193) >>> 0;
  }
  return hash;
}

/** Deterministic byte stream derived from a seed, so previews are stable across renders and devices. */
function hashBytes(seed: string, count: number): Uint8Array {
  const bytes = new Uint8Array(count);
  let round = 0;
  let offset = 0;
  while (offset < count) {
    const word = fnv1a(`${seed}\u0000${round}`, fnv1a(String(round)));
    for (let shift = 0; shift < 4 && offset < count; shift += 1) {
      bytes[offset] = (word >>> (shift * 8)) & 0xff;
      offset += 1;
    }
    round += 1;
  }
  return bytes;
}

/**
 * Code shown blurred in place of the real one: the same alphabet as a real code, shorter, and without the real
 * prefix, so it looks like a code without matching the actual one.
 */
export function contactCardPlaceholderCode(): string {
  const bytes = hashBytes(`code:${CONTACT_CARD_PLACEHOLDER_SEED}`, CONTACT_CARD_PLACEHOLDER_LENGTH);
  let text = "";
  for (const byte of bytes) text += PLACEHOLDER_CODE_ALPHABET[byte % PLACEHOLDER_CODE_ALPHABET.length];
  return text;
}

/** Whether a module belongs to one of the three finder patterns or their separators. */
function finderModule(row: number, column: number, size: number): boolean | null {
  const corners: Array<[number, number]> = [[0, 0], [0, size - FINDER_SIZE], [size - FINDER_SIZE, 0]];
  for (const [top, left] of corners) {
    const localRow = row - top;
    const localColumn = column - left;
    if (localRow < -1 || localColumn < -1 || localRow > FINDER_SIZE || localColumn > FINDER_SIZE) continue;
    if (localRow === -1 || localColumn === -1 || localRow === FINDER_SIZE || localColumn === FINDER_SIZE) return false;
    const ring = Math.min(localRow, localColumn, FINDER_SIZE - 1 - localRow, FINDER_SIZE - 1 - localColumn);
    return ring === 0 || ring >= 2;
  }
  return null;
}

/** QR-styled module grid shown blurred until the real card is revealed. The finder patterns are real; the data area is noise. */
export function placeholderCardMatrix(size = CONTACT_CARD_MATRIX_SIZE): { modules: boolean[]; width: number } {
  const bytes = hashBytes(`matrix:${CONTACT_CARD_PLACEHOLDER_SEED}`, Math.ceil((size * size) / 8));
  const modules: boolean[] = [];
  for (let row = 0; row < size; row += 1) {
    for (let column = 0; column < size; column += 1) {
      const finder = finderModule(row, column, size);
      if (finder !== null) {
        modules.push(finder);
        continue;
      }
      const index = row * size + column;
      const byte = bytes[index >> 3] ?? 0;
      modules.push(((byte >> (index & 7)) & 1) === 1);
    }
  }
  return { modules, width: size };
}
