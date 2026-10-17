import ArrowUpRight from "@lucide/svelte/icons/arrow-up-right";
import AtSign from "@lucide/svelte/icons/at-sign";
import Calendar from "@lucide/svelte/icons/calendar";
import CaseSensitive from "@lucide/svelte/icons/case-sensitive";
import CircleChevronDown from "@lucide/svelte/icons/circle-chevron-down";
import CircleDashed from "@lucide/svelte/icons/circle-dashed";
import Clock from "@lucide/svelte/icons/clock";
import FingerprintPattern from "@lucide/svelte/icons/fingerprint-pattern";
import Hash from "@lucide/svelte/icons/hash";
import Link from "@lucide/svelte/icons/link";
import List from "@lucide/svelte/icons/list";
import MapPin from "@lucide/svelte/icons/map-pin";
import MousePointerClick from "@lucide/svelte/icons/mouse-pointer-click";
import Paperclip from "@lucide/svelte/icons/paperclip";
import Phone from "@lucide/svelte/icons/phone";
import Search from "@lucide/svelte/icons/search";
import Sigma from "@lucide/svelte/icons/sigma";
import SquareCheck from "@lucide/svelte/icons/square-check";
import TextAlignStart from "@lucide/svelte/icons/text-align-start";
import UserRound from "@lucide/svelte/icons/user-round";
import Users from "@lucide/svelte/icons/users";

/** Property kinds shared by collection tables, so Notes and Projects show the same icon for the same kind of value. */
export type CollectionPropertyKind =
  | "title" | "text" | "number" | "select" | "multi_select" | "status" | "date" | "person" | "files" | "checkbox"
  | "url" | "email" | "phone" | "relation" | "rollup" | "formula" | "button" | "unique_id" | "place"
  | "created_time" | "created_by" | "last_edited_time" | "last_edited_by";

/** Lucide icon component type shared by collection menus. */
export type CollectionIcon = typeof Hash;

/** Icons for each shared property kind, used by column headers, property menus, and type pickers. */
export const COLLECTION_PROPERTY_ICONS = {
  title: CaseSensitive,
  text: TextAlignStart,
  number: Hash,
  select: CircleChevronDown,
  multi_select: List,
  status: CircleDashed,
  date: Calendar,
  person: Users,
  files: Paperclip,
  checkbox: SquareCheck,
  url: Link,
  email: AtSign,
  phone: Phone,
  relation: ArrowUpRight,
  rollup: Search,
  formula: Sigma,
  button: MousePointerClick,
  unique_id: FingerprintPattern,
  place: MapPin,
  created_time: Clock,
  created_by: UserRound,
  last_edited_time: Clock,
  last_edited_by: UserRound,
} satisfies Record<CollectionPropertyKind, CollectionIcon>;

/** A creatable property type with its label, icon kind, and picker section. */
export interface CollectionPropertyTypeOption<T extends string> {
  value: T;
  label: string;
  kind: CollectionPropertyKind;
  /** Types with the same section are listed together; sections are separated by a divider in their first-seen order. */
  section?: string;
}

/** Group type options into ordered sections and keep only those whose label matches the search text. */
export function collectionPropertyTypeSections<T extends string>(
  options: readonly CollectionPropertyTypeOption<T>[],
  search: string,
  locale?: string,
): CollectionPropertyTypeOption<T>[][] {
  const query = search.trim().toLocaleLowerCase(locale);
  const sections = new Map<string, CollectionPropertyTypeOption<T>[]>();
  for (const option of options) {
    if (query && !option.label.toLocaleLowerCase(locale).includes(query)) continue;
    const key = option.section ?? "";
    sections.set(key, [...sections.get(key) ?? [], option]);
  }
  return [...sections.values()];
}
