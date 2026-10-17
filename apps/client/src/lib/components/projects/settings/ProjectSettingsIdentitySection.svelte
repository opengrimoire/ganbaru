<script lang="ts">
  import Select from "$lib/components/ui/Select.svelte";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import { moveTextInputCaretToPointer } from "$lib/utils/text-input-caret";
  import type { ProjectLifecycleStatus } from "$lib/projects/types";
  import IconPicker from "$lib/components/icon-picker/IconPicker.svelte";
  import ProjectSettingsSectionHeading from "./ProjectSettingsSectionHeading.svelte";

  type SelectOption = { value: string; label: string };

  let {
    projectNameDraft = $bindable<string>(),
    projectGroupDraft = $bindable<string>(),
    projectStatusDraft = $bindable<ProjectLifecycleStatus>(),
    projectIconDraft = $bindable<string>(),
    projectGroupOptions,
    lifecycleOptions,
    identityLocked = false,
    setLifecycleStatus,
  }: {
    projectNameDraft: string;
    projectGroupDraft: string;
    projectStatusDraft: ProjectLifecycleStatus;
    projectIconDraft: string;
    projectGroupOptions: SelectOption[];
    lifecycleOptions: SelectOption[];
    identityLocked?: boolean;
    setLifecycleStatus: (value: string) => void;
  } = $props();

  const { t } = getLocalization();
</script>

<section class="flex flex-col gap-2">
  <div class="h-px bg-border/70" aria-hidden="true"></div>
  <div class="flex flex-col gap-1.5">
    <ProjectSettingsSectionHeading label={t("projects.settings.identity")} />
    <div class="flex flex-col gap-1.5">
      <div class="flex items-center justify-between gap-4 px-1 py-1 max-[480px]:flex-col max-[480px]:items-stretch max-[480px]:gap-2">
        <span class="min-w-0 flex-1 text-[0.866667rem] text-foreground">{t("projects.settings.name")}</span>
        <input
          bind:value={projectNameDraft}
          disabled={identityLocked}
          aria-label={t("projects.settings.name")}
          class="field w-44 min-w-0 px-2.5 text-left font-medium text-foreground disabled:opacity-100 max-[480px]:w-full"
          onpointerdown={moveTextInputCaretToPointer}
        />
      </div>

      <Select
        label={t("projects.settings.group")}
        value={projectGroupDraft}
        options={projectGroupOptions}
        disabled={identityLocked}
        onChange={(value) => {
          projectGroupDraft = value;
        }}
        class="w-44"
      />

      <Select
        label={t("projects.settings.lifecycle")}
        value={projectStatusDraft}
        options={lifecycleOptions}
        onChange={setLifecycleStatus}
        class="w-44"
      />

      <div class="flex items-center justify-between gap-4 px-1 py-1 max-[480px]:flex-col max-[480px]:items-stretch max-[480px]:gap-2">
        <div class="min-w-0 flex-1 text-[0.866667rem] text-foreground">{t("projects.settings.icon")}</div>
        <IconPicker
          value={projectIconDraft}
          ariaLabel={t("projects.settings.selectIcon", projectIconDraft)}
          class="h-7 w-44 max-[480px]:w-full"
          onChange={(nextIcon) => {
            projectIconDraft = nextIcon;
          }}
        />
      </div>
    </div>
  </div>
</section>
