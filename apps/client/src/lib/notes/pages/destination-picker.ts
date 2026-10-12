export interface NotesDestinationPickerTarget {
  key: string;
  title: string;
  path: readonly string[];
  depth: number;
  recent: boolean;
}

export interface NotesDestinationPickerSections<Target extends NotesDestinationPickerTarget> {
  recent: Target[];
  pages: Target[];
}

export type NotesDestinationPickerSectionKey = keyof NotesDestinationPickerSections<NotesDestinationPickerTarget>;

export interface NotesDestinationPickerSection<Target extends NotesDestinationPickerTarget> {
  key: NotesDestinationPickerSectionKey;
  targets: Target[];
}

function normalizedSearch(value: string): string {
  return value.trim().toLocaleLowerCase();
}

function targetMatchesQuery(target: NotesDestinationPickerTarget, query: string): boolean {
  if (!query) return true;
  return normalizedSearch([target.title, ...target.path].join(" ")).includes(query);
}

/** Split destination targets into recent and nested page sections. */
export function notesDestinationPickerSections<Target extends NotesDestinationPickerTarget>(
  targets: readonly Target[],
  query: string,
): NotesDestinationPickerSections<Target> {
  const normalizedQuery = normalizedSearch(query);
  const filtered = targets.filter((target) => targetMatchesQuery(target, normalizedQuery));
  if (normalizedQuery) {
    return {
      recent: [],
      pages: filtered,
    };
  }
  return {
    recent: filtered.filter((target) => target.recent),
    pages: filtered.filter((target) => !target.recent),
  };
}

/** Return non-empty destination picker sections in render order. */
export function notesDestinationPickerSectionList<Target extends NotesDestinationPickerTarget>(
  sections: NotesDestinationPickerSections<Target>,
): NotesDestinationPickerSection<Target>[] {
  const sectionList: NotesDestinationPickerSection<Target>[] = [
    { key: "recent", targets: sections.recent },
    { key: "pages", targets: sections.pages },
  ];
  return sectionList.filter((section) => section.targets.length > 0);
}

/** Flatten destination picker sections in keyboard navigation order. */
export function flatNotesDestinationPickerTargets<Target extends NotesDestinationPickerTarget>(
  sections: NotesDestinationPickerSections<Target>,
): Target[] {
  return notesDestinationPickerSectionList(sections).flatMap((section) => section.targets);
}

/** Return the next active option index for destination picker keyboard navigation. */
export function nextNotesDestinationPickerIndex(
  targetCount: number,
  currentIndex: number,
  key: "ArrowDown" | "ArrowUp" | "Home" | "End",
): number {
  if (targetCount <= 0) return -1;
  const safeIndex = currentIndex >= 0 && currentIndex < targetCount ? currentIndex : 0;
  if (key === "Home") return 0;
  if (key === "End") return targetCount - 1;
  if (key === "ArrowDown") return (safeIndex + 1) % targetCount;
  return (safeIndex - 1 + targetCount) % targetCount;
}
