<script lang="ts">
  import { getLocalization } from "$lib/i18n/translator.svelte";

  let {
    value,
    onChange,
  }: {
    value: string;
    onChange: (value: string) => void;
  } = $props();

  const { t } = getLocalization();
</script>

<label class="grid gap-2 text-[0.866667rem] font-semibold text-foreground">
  <span>{t("projects.detail.description")}</span>
  <textarea
    {value}
    rows="3"
    placeholder={t("projects.detail.descriptionPlaceholder")}
    class="min-h-24 resize-y rounded-md bg-transparent px-2 py-2 text-[0.866667rem] font-normal leading-relaxed text-foreground placeholder:text-muted-foreground/70 hover:bg-muted/30"
    oninput={(event) => onChange(event.currentTarget.value)}
  ></textarea>
</label>
