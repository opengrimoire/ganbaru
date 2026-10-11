<script lang="ts">
  import CollectionMenuItem from "$lib/components/collections/CollectionMenuItem.svelte";
  import CollectionMenuSelect from "$lib/components/collections/CollectionMenuSelect.svelte";
  import CollectionMenuTextField from "$lib/components/collections/CollectionMenuTextField.svelte";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import {
    notesDataSourceButtonTargetOptions,
    notesDataSourceRollupRelationOptions,
    notesDataSourceRollupTargetOptions,
    notesDataSourceRollupTargetOptionsForRelation,
    type NotesDataSourcePropertyUpdate,
    type NotesDataSourceSchemaPropertyDraft,
  } from "$lib/notes/database/data-source-schema";
  import type { NotesDataSourceOptionEdit, NotesDataSourceOptionEditor } from "$lib/notes/database/property-options";
  import {
    NOTES_DATA_SOURCE_NUMBER_FORMATS,
    NOTES_DATA_SOURCE_ROLLUP_FUNCTIONS,
    type NotesDataSource,
    type NotesDataSourcePropertyType,
  } from "$lib/notes/types";
  import NotesDatabasePropertyOptionsEditor from "./NotesDatabasePropertyOptionsEditor.svelte";

  /**
   * The type-specific settings of one data source property, shown under Edit property. Every change is handed to
   * `onUpdate` right away as an edit of the newest saved property; there is no separate save step.
   */
  let { property, properties, dataSources, dataSourceId, disabled = false, onUpdate, onOptionEdit }: {
    property: NotesDataSourceSchemaPropertyDraft;
    /** Every property of the same source, for relation, rollup, and button references. */
    properties: readonly NotesDataSourceSchemaPropertyDraft[];
    /** Sources a relation can point to, with the edited source first. */
    dataSources: readonly NotesDataSource[];
    dataSourceId: string | null;
    disabled?: boolean;
    onUpdate: (update: NotesDataSourcePropertyUpdate) => void;
    onOptionEdit: (edit: NotesDataSourceOptionEditor) => Promise<NotesDataSourceOptionEdit["status"]>;
  } = $props();

  const { t } = getLocalization();

  /** Choices for a select whose saved value may name something no longer offered, kept so the current value still shows. */
  function withCurrent(options: { value: string; label: string }[], value: string, label: string): { value: string; label: string }[] {
    return !value || options.some((option) => option.value === value) ? options : [{ value, label: label || value }, ...options];
  }

  function sourceTitle(source: NotesDataSource): string {
    return source.title.trim() || t("notes.untitled");
  }

  /** Relation properties of the related source that can mirror this relation. */
  const syncedCandidates = $derived(property.type === "relation" && property.relationDataSourceId
    ? notesDataSourceRollupTargetOptionsForRelation(property, properties, dataSourceId, dataSources)
      .filter((candidate) => candidate.type === "relation" && candidate.id !== property.id)
    : []);

  function changeRelationTarget(targetId: string): void {
    onUpdate((current) => current.relationDataSourceId === targetId
      ? null
      : { relationDataSourceId: targetId, relationSyncedPropertyId: "", relationSyncedPropertyName: "" });
  }

  function changeSyncedProperty(syncedId: string): void {
    const synced = syncedCandidates.find((candidate) => candidate.id === syncedId) ?? null;
    onUpdate(() => ({ relationSyncedPropertyId: synced?.id ?? "", relationSyncedPropertyName: synced?.name ?? "" }));
  }

  function changeRollupRelation(relationId: string): void {
    onUpdate((current, drafts) => {
      const relation = notesDataSourceRollupRelationOptions(drafts, current.id).find((candidate) => candidate.id === relationId) ?? null;
      const target = relation ? notesDataSourceRollupTargetOptionsForRelation(relation, drafts, dataSourceId, dataSources)[0] ?? null : null;
      return {
        rollupRelationPropertyId: relation?.id ?? "",
        rollupRelationPropertyName: relation?.name ?? "",
        rollupPropertyId: target?.id ?? "",
        rollupPropertyName: target?.name ?? "",
      };
    });
  }

  function changeRollupTarget(targetId: string): void {
    onUpdate((current, drafts) => {
      const target = notesDataSourceRollupTargetOptions(current, drafts, dataSourceId, dataSources).find((option) => option.id === targetId) ?? null;
      return { rollupPropertyId: target?.id ?? "", rollupPropertyName: target?.name ?? "" };
    });
  }

  function defaultButtonValue(type: NotesDataSourcePropertyType): string | boolean {
    return type === "checkbox" ? true : "";
  }

  function changeButtonTarget(targetId: string): void {
    onUpdate((current, drafts) => {
      const target = notesDataSourceButtonTargetOptions(drafts, current.id).find((candidate) => candidate.id === targetId) ?? null;
      return {
        buttonActionPropertyId: target?.id ?? "",
        buttonActionPropertyName: target?.name ?? "",
        buttonActionPropertyType: target?.type ?? "checkbox",
        buttonActionValue: target ? defaultButtonValue(target.type) : true,
      };
    });
  }

  function buttonValueText(value: unknown): string {
    return typeof value === "string" || typeof value === "number" ? String(value) : "";
  }
</script>

<div class="grid min-w-0 gap-0" data-notes-property-schema-editor={property.id}>
  {#if property.type === "select" || property.type === "multi_select" || property.type === "status"}
    <NotesDatabasePropertyOptionsEditor {property} {disabled} onEdit={onOptionEdit} />
  {:else if property.type === "number"}
    <CollectionMenuSelect label={t("notes.databaseSchemaNumberFormat")} value={property.numberFormat} {disabled}
      options={NOTES_DATA_SOURCE_NUMBER_FORMATS.map((format) => ({ value: format, label: t("notes.databaseSchemaNumberFormatLabel", format) }))}
      onChange={(numberFormat) => onUpdate(() => ({ numberFormat }))} />
  {:else if property.type === "unique_id"}
    <CollectionMenuTextField label={t("notes.databaseSchemaUniquePrefix")} value={property.uniquePrefix} placeholder={t("notes.databaseSchemaUniquePrefixPlaceholder")}
      initialFocus {disabled} onCommit={(uniquePrefix) => onUpdate(() => ({ uniquePrefix: uniquePrefix.trim() }))} />
  {:else if property.type === "relation"}
    <CollectionMenuSelect label={t("notes.databaseSchemaRelationTarget")} value={property.relationDataSourceId} {disabled}
      options={withCurrent(dataSources.map((source) => ({ value: source.id, label: sourceTitle(source) })), property.relationDataSourceId, "")}
      onChange={changeRelationTarget} />
    <CollectionMenuSelect label={t("notes.databaseSchemaRelationSyncedProperty")} value={property.relationSyncedPropertyId} {disabled}
      options={withCurrent([{ value: "", label: t("notes.databaseSchemaRelationNoSync") },
        ...syncedCandidates.map((candidate) => ({ value: candidate.id, label: candidate.name }))], property.relationSyncedPropertyId, property.relationSyncedPropertyName)}
      onChange={changeSyncedProperty} />
  {:else if property.type === "rollup"}
    {@const relations = notesDataSourceRollupRelationOptions(properties, property.id)}
    {@const targets = notesDataSourceRollupTargetOptions(property, properties, dataSourceId, dataSources)}
    <CollectionMenuSelect label={t("notes.databaseSchemaRollupRelation")} value={property.rollupRelationPropertyId} disabled={disabled || relations.length === 0}
      options={withCurrent(relations.map((relation) => ({ value: relation.id, label: relation.name })), property.rollupRelationPropertyId, property.rollupRelationPropertyName)}
      onChange={changeRollupRelation} />
    <CollectionMenuSelect label={t("notes.databaseSchemaRollupTargetProperty")} value={property.rollupPropertyId} disabled={disabled || targets.length === 0}
      options={withCurrent(targets.map((target) => ({ value: target.id, label: target.name })), property.rollupPropertyId, property.rollupPropertyName)}
      onChange={changeRollupTarget} />
    <CollectionMenuSelect label={t("notes.databaseSchemaRollupFunction")} value={property.rollupFunction} {disabled}
      options={NOTES_DATA_SOURCE_ROLLUP_FUNCTIONS.map((rollupFunction) => ({ value: rollupFunction, label: t("notes.databaseSchemaRollupFunctionLabel", rollupFunction) }))}
      onChange={(rollupFunction) => onUpdate(() => ({ rollupFunction }))} />
    {#if relations.length === 0}<p class="px-2 py-1 text-panel-detail text-muted-foreground">{t("notes.databaseSchemaRollupNoRelations")}</p>{/if}
  {:else if property.type === "formula"}
    <CollectionMenuTextField label={t("notes.databaseSchemaFormulaExpression")} value={property.formulaExpression} placeholder={t("notes.databaseSchemaFormulaExpressionPlaceholder")}
      multiline monospace initialFocus {disabled} onCommit={(formulaExpression) => onUpdate(() => ({ formulaExpression: formulaExpression.trim() }))} />
  {:else if property.type === "button"}
    {@const targets = notesDataSourceButtonTargetOptions(properties, property.id)}
    <CollectionMenuTextField label={t("notes.databaseSchemaButtonLabel")} value={property.buttonLabel} placeholder={t("notes.databaseSchemaButtonDefaultLabel")}
      initialFocus {disabled} onCommit={(buttonLabel) => onUpdate(() => ({ buttonLabel: buttonLabel.trim() }))} />
    <CollectionMenuSelect label={t("notes.databaseSchemaButtonTarget")} value={property.buttonActionPropertyId} {disabled}
      options={withCurrent([{ value: "", label: t("notes.databaseSchemaButtonNoAction") }, ...targets.map((target) => ({ value: target.id, label: target.name }))],
        property.buttonActionPropertyId, property.buttonActionPropertyName)}
      onChange={changeButtonTarget} />
    {#if property.buttonActionPropertyId}
      {#if property.buttonActionPropertyType === "checkbox"}
        <CollectionMenuSelect label={t("notes.databaseSchemaButtonValue")} value={property.buttonActionValue === false ? "false" : "true"} {disabled}
          options={[{ value: "true", label: t("notes.databaseSchemaButtonValueChecked") }, { value: "false", label: t("notes.databaseSchemaButtonValueUnchecked") }]}
          onChange={(value) => onUpdate(() => ({ buttonActionValue: value === "true" }))} />
      {:else}
        <CollectionMenuTextField label={t("notes.databaseSchemaButtonValue")} value={buttonValueText(property.buttonActionValue)} placeholder={t("notes.databaseSchemaButtonValuePlaceholder")}
          {disabled} onCommit={(buttonActionValue) => onUpdate(() => ({ buttonActionValue }))} />
      {/if}
    {/if}
    <CollectionMenuItem label={t("notes.databaseSchemaButtonConfirm")} checked={property.buttonRequiresConfirmation} {disabled}
      onclick={() => onUpdate((current) => ({ buttonRequiresConfirmation: !current.buttonRequiresConfirmation }))} />
  {/if}
</div>
