import type {
  NotesWorkingMarkdownFileRef,
  NotesWorkingMarkdownTreeRead,
} from "$lib/notes/types";

export interface NotesWorkingMarkdownTreeProps {
  tree: NotesWorkingMarkdownTreeRead;
  query: string;
  selectedFile: NotesWorkingMarkdownFileRef | null;
  loading: boolean;
  error: string | null;
  onSelect: (file: NotesWorkingMarkdownFileRef) => void;
  onRefresh: () => void;
}
