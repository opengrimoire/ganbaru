import type { NotesBlock, NotesBlockType, NotesFolder, NotesPage, NotesParent } from "./core";
import type { NotesPageBreadcrumbItem } from "./knowledge";

export interface NotesSidebarPagesRequest {
  expanded_page_ids: string[];
  seed_page_ids: string[];
  selected_page_id: string | null;
}

export interface NotesSidebarPageList {
  pages: NotesPage[];
  page_ids_with_children: string[];
  missing_parent_page_ids: string[];
  trashed_parent_page_ids: string[];
}

export interface NotesPageSummaryWindowRequest {
  cursor?: string | null;
  query?: string | null;
  page_size?: number | null;
}

export interface NotesPageSummaryWindow {
  pages: NotesPage[];
  total_count: number;
  next_cursor: string | null;
}

export interface NotesWorkspaceShellRequest {
  project_id: string | null;
  expanded_page_ids: string[];
  seed_page_ids: string[];
  selected_page_id: string | null;
  page_cursor?: string | null;
  folder_cursor?: string | null;
  destination_candidates?: boolean;
  page_query?: string | null;
  include_navigation_index?: boolean;
}

export interface NotesWorkspaceShell {
  pages: NotesPage[];
  folders: NotesFolder[];
  navigation_pages: NotesPage[];
  navigation_folders: NotesFolder[];
  navigation_page_ids_with_children: string[];
  navigation_databases: NotesNavigationDatabase[];
  page_ids_with_children: string[];
  missing_parent_page_ids: string[];
  trashed_parent_page_ids: string[];
  resolved_selected_page_id: string | null;
  total_page_count: number;
  total_folder_count: number;
  next_page_cursor: string | null;
  next_folder_cursor: string | null;
}

/** Database identity used by hierarchy pickers without retaining rows or schemas. */
export interface NotesNavigationDatabase {
  id: string;
  page_id: string;
  title: string;
  data_source_id: string;
}

export interface NotesPaginatedBlockList {
  object: "list";
  type: "block";
  block: Record<string, unknown>;
  results: NotesBlock[];
  next_cursor: string | null;
  has_more: boolean;
}

export interface NotesLoadedPage {
  page: NotesPage;
  blocks: NotesPaginatedBlockList;
}

export interface NotesPageOpenResponse extends NotesLoadedPage {
  breadcrumb: NotesPageBreadcrumbItem[];
  outlines: NotesBlockOutline[];
}

export interface NotesBlockOutline {
  ganbaru_indent?: number;
  id: string;
  page_id: string;
  parent: Extract<NotesParent, { type: "page_id" | "block_id" }>;
  type: NotesBlockType;
  sort_order: number;
  has_children: boolean;
  retained_height: number;
}

export interface NotesBlockHydrationRequest {
  page_id: string;
  block_ids: string[];
}

export interface NotesBlockFrontier {
  blocks: NotesBlock[];
}
