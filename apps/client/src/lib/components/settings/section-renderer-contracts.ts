import type {
  ChatProviderSetupTarget,
  AgentsSettingsSubsection,
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
  readonly activeAgentsSubsection: AgentsSettingsSubsection;
  readonly initialAgentsTeammateId?: string;
  readonly initialAgentsChannelId?: string;
  readonly initialAgentsCreateTeammate?: boolean;
  readonly onOpenDistractionsLimitEditor: (target: DistractionsLimitEditorTarget) => void;
  readonly onOpenNotesTransferPanel: (operation: NotesTransferOperation) => void;
  readonly onOpenChatProviderSetup: (target: ChatProviderSetupTarget) => void;
  readonly onAgentsSubsectionChange: (subsection: AgentsSettingsSubsection) => void;
  readonly onRequestNavigation: (navigate: () => void) => void;
  readonly onDraftStateChange: (draft: SettingsDraftKind | null) => void;
}
