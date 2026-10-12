import {
  beginLazyComponentLoad,
  rejectLazyComponentLoad,
  resolveLazyComponentLoad,
  type LazyComponentLoadState,
} from "$lib/lazy-component-loader";
import {
  loadNotesAdvancedBlock,
  retryNotesAdvancedBlock,
  type LoadedNotesAdvancedBlock,
  type NotesAdvancedBlockFamily,
} from "$lib/components/notes/editor-component-registry";

type NotesStructuralBlockFamily = "column-list" | "tab";
type StructuralLoadState = LazyComponentLoadState<
  NotesAdvancedBlockFamily,
  LoadedNotesAdvancedBlock
>;

/** Own the shared lazy load state for recursive Notes structural renderers. */
export function createNotesStructuralBlockLoader() {
  let states = $state<Partial<Record<NotesAdvancedBlockFamily, StructuralLoadState>>>({});

  function request(kind: NotesStructuralBlockFamily, retry = false): void {
    const current = states[kind] ?? null;
    if (!retry && current?.key === kind) return;
    const loadingState = beginLazyComponentLoad(current, kind);
    states = { ...states, [kind]: loadingState };
    const pending = retry ? retryNotesAdvancedBlock(kind) : loadNotesAdvancedBlock(kind);
    void pending.then((component) => {
      const latest = states[kind];
      if (!latest) return;
      states = {
        ...states,
        [kind]: resolveLazyComponentLoad(latest, kind, loadingState.requestId, component),
      };
    }).catch((error: unknown) => {
      const latest = states[kind];
      if (!latest) return;
      states = {
        ...states,
        [kind]: rejectLazyComponentLoad(latest, kind, loadingState.requestId, error),
      };
      console.error(`load Notes ${kind} block failed`, error);
    });
  }

  return {
    stateFor(kind: NotesStructuralBlockFamily): StructuralLoadState | null {
      return states[kind] ?? null;
    },
    request,
  };
}
