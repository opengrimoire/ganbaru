<script lang="ts">
  import AppearanceSection from "$lib/components/settings/sections/AppearanceSection.svelte";
  import ProfileSection from "$lib/components/settings/sections/ProfileSection.svelte";
  import ContactsSection from "$lib/components/settings/sections/ContactsSection.svelte";
  import CalendarsSection from "$lib/components/settings/sections/CalendarsSection.svelte";
  import ProjectsSection from "$lib/components/settings/sections/ProjectsSection.svelte";
  import NotesSection from "$lib/components/settings/sections/NotesSection.svelte";
  import AboutSection from "$lib/components/settings/sections/AboutSection.svelte";
  import FocusSection from "$lib/components/settings/sections/FocusSection.svelte";
  import DistractionsSection from "$lib/components/settings/distractions/DistractionsSection.svelte";
  import MobileDataSection from "$lib/components/settings/mobile/MobileDataSection.svelte";
  import type { SettingsSectionRendererProps } from "./section-renderer-contracts";

  let {
    activeSection,
    initialDistractionsTab,
    initialContactsTab,
    onOpenDistractionsLimitEditor,
    onDraftStateChange,
  }: SettingsSectionRendererProps = $props();

</script>

{#if activeSection === "appearance"}
  <AppearanceSection />
{:else if activeSection === "profile"}
  <ProfileSection {onDraftStateChange} />
{:else if activeSection === "contacts"}
  <ContactsSection initialTab={initialContactsTab} />
{:else if activeSection === "calendars"}
  <CalendarsSection fileTransfersAvailable={__GANBARU_AI_BUILD_PLATFORM__ === "android"} />
{:else if activeSection === "projects"}
  <ProjectsSection />
{:else if activeSection === "notes"}
  <NotesSection transfersAvailable={false} notificationsAvailable={false} />
{:else if activeSection === "chat"}
  {#await import("$lib/components/settings/mobile/MobileChatSection.svelte") then module}
    {@const MobileChatSection = module.default}
    <MobileChatSection />
  {/await}
{:else if activeSection === "focus"}
  <FocusSection>
    {#snippet backgroundExecutionSettings()}
      {#await import("$lib/components/settings/sections/AndroidBackgroundExecutionSettings.svelte") then module}
        {@const AndroidBackgroundExecutionSettings = module.default}
        <AndroidBackgroundExecutionSettings />
      {/await}
    {/snippet}
  </FocusSection>
{:else if activeSection === "music"}
  {#await import("$lib/components/settings/sections/MusicSection.svelte") then module}
    {@const MusicSettings = module.default}
    <MusicSettings />
  {/await}
{:else if activeSection === "distractions"}
  <DistractionsSection
    initialTab={initialDistractionsTab}
    onOpenLimitEditor={onOpenDistractionsLimitEditor}
  />
{:else if activeSection === "data"}
  <MobileDataSection />
{:else if activeSection === "updates"}
  {#await import("$lib/components/settings/mobile/MobileUpdatesSection.svelte") then module}
    {@const MobileUpdatesSection = module.default}
    <MobileUpdatesSection />
  {/await}
{:else if activeSection === "about"}
  <AboutSection />
{/if}
