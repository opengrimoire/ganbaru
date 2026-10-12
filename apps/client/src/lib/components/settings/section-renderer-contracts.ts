import type {
  ChatProviderSetupTarget,
  ChatSettingsSubsection,
  DistractionsLimitEditorTarget,
  DistractionsSettingsTab,
  NotesTransferOperation,
  ContactsSettingsTab,
  SectionId,
  SettingsDraftKind,
} from "$lib/settings/types";

/** Props shared by the compile-time desktop and mobile settings renderers. */
export interface SettingsSectionRendererProps {
  readonly activeSection: SectionId;
  readonly initialDistractionsTab?: DistractionsSettingsTab;
  readonly initialContactsTab?: ContactsSettingsTab;
  readonly activeChatSubsection: ChatSettingsSubsection;
  readonly initialChatTeammateId?: string;
  readonly initialChatChannelId?: string;
  readonly initialChatCreateTeammate?: boolean;
  readonly onOpenDistractionsLimitEditor: (target: DistractionsLimitEditorTarget) => void;
  readonly onOpenNotesTransferPanel: (operation: NotesTransferOperation) => void;
  readonly onOpenChatProviderSetup: (target: ChatProviderSetupTarget) => void;
  readonly onChatSubsectionChange: (subsection: ChatSettingsSubsection) => void;
  readonly onRequestNavigation: (navigate: () => void) => void;
  readonly onDraftStateChange: (draft: SettingsDraftKind | null) => void;
}
