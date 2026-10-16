import type { View } from "$lib/navigation";
import type { ProjectHostedViewType } from "$lib/projects/types";

/** What the phone shell knows about the Projects tab when choosing the workspace it shows. */
export interface MobileProjectHostingState {
  /** Hosted view type of the selected project's open view, or null while a task view is open. */
  hostedPlace: ProjectHostedViewType | null;
  /** Whether a project and its group are selected; hosting needs them to scope Notes or Chat. */
  projectSelected: boolean;
}

/**
 * Returns the workspace the phone shows for the current tab. The Projects tab shows the selected project's Notes or
 * Chat in place of a task view when that view is open, as the desktop does, so the tab keeps the user inside the
 * project.
 */
export function mobileWorkspaceSurface(current: View, hosting: MobileProjectHostingState | null): View {
  if (current !== "projects" || !hosting?.hostedPlace || !hosting.projectSelected) return current;
  return hosting.hostedPlace;
}
