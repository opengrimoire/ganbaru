/** What decides whether a collapsible Chat composer rests as a single row. */
export interface ComposerCollapseState {
  /** Whether this composer may collapse at all, which only the phone shell allows. */
  collapsible: boolean;
  /** Whether focus is in the editor or one of the composer's controls. */
  focusWithin: boolean;
  /** Whether the draft has text, references, or attachments. */
  hasContent: boolean;
  /** Whether a send option is set, such as a schedule or sharing a reply to the channel. */
  optionSet: boolean;
  /** Whether a menu, picker, or panel of the composer is open. */
  menuOpen: boolean;
  /** Whether the composer shows a notice, a blocker, an error, or an edit in progress. */
  showsStatus: boolean;
}

/**
 * Return whether a composer rests as a single row with only its first tool and Send: it collapses only when nothing
 * in it needs the room, so a draft, a chosen option, an open menu, or a message about the draft keeps it open.
 */
export function composerCollapsed(state: ComposerCollapseState): boolean {
  return state.collapsible
    && !state.focusWithin
    && !state.hasContent
    && !state.optionSet
    && !state.menuOpen
    && !state.showsStatus;
}
