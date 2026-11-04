<script lang="ts">
  import BriefcaseBusiness from "@lucide/svelte/icons/briefcase-business";
  import FolderX from "@lucide/svelte/icons/folder-x";
  import Notice from "$lib/components/ui/Notice.svelte";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import { useProjects } from "$lib/stores/projects.svelte";
  import { cn } from "$lib/utils";
  import ProjectNavigator from "./ProjectNavigator.svelte";
  import ProjectPickerMobileDialog from "$lib/components/projects/pickers/ProjectPickerMobileDialog.svelte";

  let {
    selectedProjectId,
    showInactiveProjects = $bindable<boolean>(),
    mobileLayout = false,
    onProjectSelected,
  }: {
    selectedProjectId: string | null;
    showInactiveProjects: boolean;
    mobileLayout?: boolean;
    onProjectSelected: () => void;
  } = $props();

  const projects = useProjects();
  const { t } = getLocalization();

  let navigatorOpen = $state(false);

  function closeNavigator(): void {
    navigatorOpen = false;
  }

</script>

<div
  class="flex h-full flex-col items-center justify-center gap-3 p-4 text-center text-[0.866667rem] text-muted-foreground"
  data-projects-first-use-state
>
  {#if projects.loadError}
    <Notice
      layout="fill"
      icon={FolderX}
      message={t("projects.loadFailed", projects.loadError)}
      onRetry={() => projects.load()}
    />
  {:else if projects.loaded}
    <div>{t("projects.navigator.empty")}</div>
    <button
      type="button"
      class={cn(
        "flex items-center gap-1.5 rounded-md border border-border bg-background px-3 text-[0.8rem] font-medium text-foreground hover:bg-accent",
        mobileLayout ? "min-h-12" : "min-h-9",
      )}
      onclick={() => {
        navigatorOpen = !navigatorOpen;
      }}
    >
      <BriefcaseBusiness size={14} strokeWidth={1.75} />
      <span>{t("projects.navigator.open")}</span>
    </button>
    {#if navigatorOpen}
      {#if mobileLayout}
        <ProjectPickerMobileDialog
          label={t("projects.navigator.pickerLabel")}
          closeLabel={t("projects.navigator.closePicker")}
          onClose={closeNavigator}
        >
          <div class="h-full text-left">
            <ProjectNavigator
              {selectedProjectId}
              {showInactiveProjects}
              onShowInactiveProjectsChange={(value) => {
                showInactiveProjects = value;
              }}
              onProjectSelected={() => {
                closeNavigator();
                onProjectSelected();
              }}
              mobileLayout
              onClose={closeNavigator}
            />
          </div>
        </ProjectPickerMobileDialog>
      {:else}
        <div class="h-[min(24rem,70vh)] w-[min(28rem,100%)] text-left">
          <ProjectNavigator
            {selectedProjectId}
            {showInactiveProjects}
            onShowInactiveProjectsChange={(value) => {
              showInactiveProjects = value;
            }}
            onProjectSelected={() => {
              closeNavigator();
              onProjectSelected();
            }}
            onClose={closeNavigator}
          />
        </div>
      {/if}
    {/if}
  {/if}
</div>
