<!--
  @component
  The model the new work of a post runs with: the teammate default, or one visible model of the
  teammate's provider. It is disabled with a reason where a post cannot carry an override.
-->
<script lang="ts">
  import type { ChatTeammatePolicyRead, ProviderInstanceRead } from "$lib/chat/contracts";
  import { visibleProviderModels } from "$lib/chat/composer/model-picker-model";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import Select from "$lib/components/ui/Select.svelte";

  let {
    provider,
    policy,
    modelId,
    disabledReason,
    onChange,
  }: {
    /** The provider instance the teammate's policy runs on, when it is configured here. */
    provider: ProviderInstanceRead | null;
    policy: ChatTeammatePolicyRead;
    /** The chosen model, or `null` for the teammate default. */
    modelId: string | null;
    /** Why no model can be chosen for this post, or `null` when one can. */
    disabledReason: string | null;
    onChange: (modelId: string | null) => void;
  } = $props();

  const { t } = getLocalization();
  const DEFAULT_VALUE = "";
  const options = $derived([
    { value: DEFAULT_VALUE, label: t("chat.composer.modelOverride.teammateDefault") },
    ...visibleProviderModels(provider, {
      providerInstanceId: policy.providerInstanceId,
      modelId,
      providerManaged: false,
    }).map((model) => ({ value: model.id, label: model.displayName })),
  ]);
  const label = $derived(disabledReason
    ? `${t("chat.composer.modelOverride.label")}: ${disabledReason}`
    : t("chat.composer.modelOverride.label"));
</script>

<span class="model-override" title={disabledReason ?? undefined} data-chat-model-override>
  <Select
    inline
    appearance="quiet"
    class="model-override-select"
    ariaLabel={label}
    value={disabledReason ? DEFAULT_VALUE : modelId ?? DEFAULT_VALUE}
    {options}
    disabled={disabledReason !== null || options.length < 2}
    onChange={(value) => onChange(value === DEFAULT_VALUE ? null : value)}
  />
</span>

<style>
  .model-override { display: inline-flex; min-width: 0; }
  .model-override :global(.model-override-select) { width: auto; max-width: 10rem; }
</style>
