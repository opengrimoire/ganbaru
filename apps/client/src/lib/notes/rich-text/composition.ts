export interface NotesCompositionSignal {
  active: boolean;
  eventIsComposing: boolean;
}

export interface NotesCompositionKeySignal extends NotesCompositionSignal {
  key: string;
}

/**
 * Return whether rich editor input should wait for the IME commit event.
 */
export function shouldDeferNotesCompositionInput(input: NotesCompositionSignal): boolean {
  return input.active || input.eventIsComposing;
}

/**
 * Return whether a key event belongs to the native IME session.
 */
export function shouldLetNativeCompositionHandleKeydown(
  input: NotesCompositionKeySignal,
): boolean {
  return input.active || input.eventIsComposing || input.key === "Process";
}
