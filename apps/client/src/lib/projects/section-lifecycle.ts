import type { ProjectLifecycleStatus, ProjectSection, SectionLifecycleChange } from "$lib/projects/types";

/** The lifecycle a section is in: hidden while `hiddenAt` is set and archived while `archivedAt` is. */
export function sectionLifecycle(section: Pick<ProjectSection, "hiddenAt" | "archivedAt">): ProjectLifecycleStatus {
  if (section.archivedAt) return "archived";
  return section.hiddenAt ? "hidden" : "active";
}

/**
 * The move that takes a section to `next` from the state it was read in, or `null` when it is
 * already there.
 */
export function sectionLifecycleChange(
  section: Pick<ProjectSection, "id" | "hiddenAt" | "archivedAt">,
  next: ProjectLifecycleStatus,
): SectionLifecycleChange | null {
  const expected = sectionLifecycle(section);
  if (expected === next) return null;
  return { sectionId: section.id, expected, next };
}
