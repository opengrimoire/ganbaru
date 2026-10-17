<script lang="ts">
  import type { Component } from "svelte";
  import ShieldCheck from "@lucide/svelte/icons/shield-check";
  import ShieldX from "@lucide/svelte/icons/shield-x";
  import type { DistractionsMode } from "$lib/distractions";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import { cn } from "$lib/utils";

  interface ModeOption {
    mode: DistractionsMode;
    label: string;
    description: string;
    icon: Component;
  }

  let {
    mode,
    blacklistDescription,
    whitelistDescription,
    onChange,
  }: {
    mode: DistractionsMode;
    blacklistDescription: string;
    whitelistDescription: string;
    onChange: (mode: DistractionsMode) => void;
  } = $props();

  const { t } = getLocalization();

  const options = $derived<readonly ModeOption[]>([
    {
      mode: "blacklist",
      label: t("settings.distractions.browser.blacklistMode"),
      description: blacklistDescription,
      icon: ShieldX,
    },
    {
      mode: "whitelist",
      label: t("settings.distractions.browser.whitelistMode"),
      description: whitelistDescription,
      icon: ShieldCheck,
    },
  ]);
</script>

<div class="grid grid-cols-2 items-start gap-2 max-[560px]:grid-cols-1">
  {#each options as option}
    {@const Icon = option.icon}
    {@const active = mode === option.mode}
    <button
      type="button"
      onclick={() => onChange(option.mode)}
      class={cn(
        "flex min-h-0 w-full items-start gap-2.5 rounded-md border px-3 py-2 text-left leading-normal disabled:cursor-not-allowed max-[360px]:gap-2 max-[360px]:px-2.5",
        active
          ? "border-foreground/25 bg-foreground/5 text-foreground"
          : "border-border/60 bg-transparent text-muted-foreground opacity-70",
      )}
      aria-pressed={active}
    >
      <Icon size={16} strokeWidth={2} class="mt-0.5 shrink-0" />
      <span class="min-w-0 flex-1">
        <span class="block text-[0.866667rem] font-medium leading-snug">{option.label}</span>
        <span class="mt-0.5 block text-[0.8rem] leading-snug text-muted-foreground">{option.description}</span>
      </span>
    </button>
  {/each}
</div>
