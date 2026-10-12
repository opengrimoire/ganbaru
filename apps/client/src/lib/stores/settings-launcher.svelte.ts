/**
 * Cross-component launcher for the Settings modal.
 *
 * The modal itself is mounted once in `TitleBarOverlayHost.svelte`; any
 * component (the calendar header, the calendar sidebar popover, other feature
 * surfaces) can request that it open targeted at a specific section instead of having
 * to reach back into the title bar for state.
 *
 * The store deliberately exposes a tiny API: callers say `open("calendars")`
 * and the title bar reacts. The `targetSection` is only consumed when the
 * modal mounts; clearing it on `close()` keeps re-opens (gear icon) free of
 * stale targeting. A caller that sends the user to Settings mid-task passes
 * `onClosed` to restore its own surface once the modal is gone.
 */

import type { ChatSettingsSubsection, DistractionsSettingsTab, ContactsSettingsTab, SectionId } from "$lib/settings/types";

interface SettingsLaunchOptions {
  distractionsTab?: DistractionsSettingsTab;
  contactsTab?: ContactsSettingsTab;
  chatSubsection?: ChatSettingsSubsection;
  chatTeammateId?: string;
  chatChannelId?: string;
  chatCreateTeammate?: boolean;
  /** Runs once after the modal closes, however it closes, so the opener can bring back what it had to hide. */
  onClosed?: () => void;
}

class SettingsLauncherStore {
  isOpen = $state(false);
  targetSection = $state<SectionId | undefined>(undefined);
  targetDistractionsTab = $state<DistractionsSettingsTab | undefined>(undefined);
  targetContactsTab = $state<ContactsSettingsTab | undefined>(undefined);
  targetChatSubsection = $state<ChatSettingsSubsection | undefined>(undefined);
  targetChatTeammateId = $state<string | undefined>(undefined);
  targetChatChannelId = $state<string | undefined>(undefined);
  targetChatCreateTeammate = $state(false);
  private onClosed: (() => void) | null = null;

  /**
   * Request that the Settings modal open. Pass `section` to land on a
   * specific section; omit it to keep the previously selected section
   * (defaulting to Appearance on first open).
   */
  open(section?: SectionId, options: SettingsLaunchOptions = {}) {
    this.targetSection = section;
    this.targetDistractionsTab = section === "distractions"
      ? options.distractionsTab
      : undefined;
    this.targetContactsTab = section === "contacts" ? options.contactsTab : undefined;
    this.targetChatSubsection = section === "chat" ? options.chatSubsection : undefined;
    this.targetChatTeammateId = section === "chat" ? options.chatTeammateId : undefined;
    this.targetChatChannelId = section === "chat" ? options.chatChannelId : undefined;
    this.targetChatCreateTeammate = section === "chat" ? options.chatCreateTeammate ?? false : false;
    this.onClosed = options.onClosed ?? null;
    this.isOpen = true;
  }

  close() {
    const onClosed = this.onClosed;
    this.onClosed = null;
    this.isOpen = false;
    this.targetSection = undefined;
    this.targetDistractionsTab = undefined;
    this.targetContactsTab = undefined;
    this.targetChatSubsection = undefined;
    this.targetChatTeammateId = undefined;
    this.targetChatChannelId = undefined;
    this.targetChatCreateTeammate = false;
    onClosed?.();
  }
}

let store: SettingsLauncherStore | null = null;

export function getSettingsLauncher(): SettingsLauncherStore {
  if (!store) store = new SettingsLauncherStore();
  return store;
}
