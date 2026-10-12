<script lang="ts">
  import type { Component } from "svelte";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import AppearanceSection from "$lib/components/settings/sections/AppearanceSection.svelte";
  import ProfileSection from "$lib/components/settings/sections/ProfileSection.svelte";
  import ContactsSection from "$lib/components/settings/sections/ContactsSection.svelte";
  import CalendarsSection from "$lib/components/settings/sections/CalendarsSection.svelte";
  import ProjectsSection from "$lib/components/settings/sections/ProjectsSection.svelte";
  import NotesSection from "$lib/components/settings/sections/NotesSection.svelte";
  import ChatSection from "$lib/components/settings/sections/ChatSection.svelte";
  import FocusSection from "$lib/components/settings/sections/FocusSection.svelte";
  import MusicSection from "$lib/components/settings/sections/MusicSection.svelte";
  import DistractionsSection from "$lib/components/settings/distractions/DistractionsSection.svelte";
  import UpdatesSection from "$lib/components/settings/sections/UpdatesSection.svelte";
  import ShortcutsSection from "$lib/components/settings/sections/ShortcutsSection.svelte";
  import AboutSection from "$lib/components/settings/sections/AboutSection.svelte";
  import Notice from "$lib/components/ui/Notice.svelte";
  import {
    getPreloadedDataSection,
    preloadDataSection,
  } from "./section-catalog";
  import type { SectionId } from "$lib/settings/types";
  import type { SettingsSectionRendererProps } from "./section-renderer-contracts";

  let {
    activeSection,
    initialDistractionsTab,
    initialContactsTab,
    activeChatSubsection,
    initialChatTeammateId,
    initialChatChannelId,
    initialChatCreateTeammate,
    onOpenDistractionsLimitEditor,
    onOpenNotesTransferPanel,
    onOpenChatProviderSetup,
    onChatSubsectionChange,
    onRequestNavigation,
    onDraftStateChange,
  }: SettingsSectionRendererProps = $props();

  const { t } = getLocalization();
  let DataSection = $state<Component | null>(getPreloadedDataSection());
  let dataSectionLoadFailed = $state(false);

  /** Loads the Data section chunk; a failure is logged and recorded for the retry notice. */
  async function loadDataSection(): Promise<void> {
    dataSectionLoadFailed = false;
    try {
      DataSection = await preloadDataSection();
    } catch (error: unknown) {
      dataSectionLoadFailed = true;
      console.error("Failed to load the Data Settings section", error);
    }
  }

  /** Retries the Data section load and rejects when it fails again, so the notice shows the outcome. */
  async function retryDataSection(): Promise<void> {
    await loadDataSection();
    if (dataSectionLoadFailed) throw new Error("The Data Settings section could not be loaded");
  }

  if (getPreloadedDataSection() === null) void loadDataSection();

  const BASIC_SECTION_COMPONENTS: Partial<Record<SectionId, Component>> = {
    appearance: AppearanceSection,
    calendars: CalendarsSection,
    projects: ProjectsSection,
    focus: FocusSection,
    music: MusicSection,
    updates: UpdatesSection,
    shortcuts: ShortcutsSection,
    about: AboutSection,
  };

  const activeBasicSection = $derived(BASIC_SECTION_COMPONENTS[activeSection]);
</script>

{#if activeSection === "profile"}
  <ProfileSection {onDraftStateChange} />
{:else if activeSection === "contacts"}
  <ContactsSection initialTab={initialContactsTab} />
{:else if activeSection === "notes"}
  <NotesSection onOpenTransferPanel={onOpenNotesTransferPanel} />
{:else if activeSection === "distractions"}
  <DistractionsSection
    initialTab={initialDistractionsTab}
    onOpenLimitEditor={onOpenDistractionsLimitEditor}
  />
{:else if activeSection === "chat"}
  <ChatSection
    initialSubsection={activeChatSubsection}
    {initialChatTeammateId}
    {initialChatChannelId}
    {initialChatCreateTeammate}
    onOpenProviderSetup={onOpenChatProviderSetup}
    onSubsectionChange={onChatSubsectionChange}
    {onRequestNavigation}
    onTeammateDraftStateChange={(open: boolean) => onDraftStateChange(open ? "teammate" : null)}
  />
{:else if activeSection === "data"}
  {#if DataSection}
    <DataSection />
  {:else if dataSectionLoadFailed}
    <div class="min-h-36 text-sm">
      <Notice layout="fill" message={t("common.viewLoadFailed", t("settings.section.data"))} onRetry={retryDataSection} />
    </div>
  {:else}
    <div class="flex min-h-36 items-center justify-center text-sm text-muted-foreground" aria-busy="true">
      {t("common.loading")}
    </div>
  {/if}
{:else if activeBasicSection}
  {@const SectionComponent = activeBasicSection}
  <SectionComponent />
{/if}
