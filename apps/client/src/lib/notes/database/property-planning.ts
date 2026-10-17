import type { NotesDataSource, NotesDataSourcePropertyType } from "$lib/notes/types";
import {
  createNotesDataSourcePropertyDraft,
  notesDataSourceDefaultButtonPatch,
  notesDataSourceDefaultRollupPatch,
  notesDataSourcePropertyConfig,
  notesDataSourceSchemaUpdateFromDraft,
  type NotesDataSourceSchemaPropertyDraft,
} from "./data-source-schema";

/** A planned property ready to save, or why its type cannot be configured from the current schema. */
export type NotesDataSourcePropertyPlan =
  | { status: "ready"; property: NotesDataSourceSchemaPropertyDraft }
  | { status: "rollup_without_relation" };

/** The native configuration a type change sends, or why the new type cannot be configured. */
export type NotesDataSourceTypeChangePlan =
  | { status: "ready"; config: Record<string, unknown> | null }
  | { status: "rollup_without_relation" };

/** Input shared by plans that build a property inside one source. */
export interface NotesDataSourcePropertyPlanContext {
  /** Source that owns the property. */
  dataSourceId: string;
  /** Newest saved drafts of that source. */
  drafts: readonly NotesDataSourceSchemaPropertyDraft[];
  /** Sources a rollup can read through, used to choose its default target. */
  dataSources: readonly NotesDataSource[];
}

/**
 * Fill the references a property of its type needs to be valid: a relation points to its own source, a rollup to the
 * first relation, a formula to the first plain property, and a button to the first settable property.
 *
 * @param property Draft to complete; it is copied, not changed.
 * @param context Source, saved drafts, and available sources.
 * @returns The completed draft, or a rollup error when the source has no relation to read through.
 */
export function planNotesDataSourcePropertyDefaults(
  property: NotesDataSourceSchemaPropertyDraft,
  context: NotesDataSourcePropertyPlanContext,
): NotesDataSourcePropertyPlan {
  const next = { ...property };
  switch (next.type) {
    case "relation":
      next.relationDataSourceId = context.dataSourceId;
      break;
    case "rollup":
      Object.assign(next, notesDataSourceDefaultRollupPatch(context.drafts, next.id, context.dataSourceId, context.dataSources));
      if (!next.rollupRelationPropertyId) return { status: "rollup_without_relation" };
      break;
    case "formula":
      next.formulaExpression = notesDataSourceDefaultFormulaExpression(next.id, context.drafts);
      break;
    case "button":
      Object.assign(next, notesDataSourceDefaultButtonPatch(context.drafts, next.id));
      break;
    default:
      break;
  }
  return { status: "ready", property: next };
}

/**
 * Plan a new property of one type with its type defaults.
 *
 * @param type Property type.
 * @param name Trimmed name, already checked for uniqueness by the caller or the native schema.
 * @param icon `lucide:` picker value, or null for the type icon.
 * @param context Source, saved drafts, and available sources.
 */
export function planNewNotesDataSourceProperty(
  type: NotesDataSourcePropertyType,
  name: string,
  icon: string | null,
  context: NotesDataSourcePropertyPlanContext,
): NotesDataSourcePropertyPlan {
  return planNotesDataSourcePropertyDefaults(createNotesDataSourcePropertyDraft(type, name, undefined, icon), context);
}

/**
 * Plan the configuration a property type change sends. Select-like types collect their options from the existing
 * values natively, so they send none; other types send their defaults for the property's name and icon.
 *
 * @param current Saved draft of the property being converted.
 * @param propertyType Type it converts to.
 * @param context Source, saved drafts, and available sources.
 */
export function planNotesDataSourceTypeChange(
  current: NotesDataSourceSchemaPropertyDraft,
  propertyType: NotesDataSourcePropertyType,
  context: NotesDataSourcePropertyPlanContext,
): NotesDataSourceTypeChangePlan {
  if (propertyType === "select" || propertyType === "multi_select" || propertyType === "status") {
    return { status: "ready", config: null };
  }
  const plan = planNotesDataSourcePropertyDefaults(
    createNotesDataSourcePropertyDraft(propertyType, current.name, current.id, current.icon),
    context,
  );
  return plan.status === "ready" ? { status: "ready", config: notesDataSourcePropertyConfig(plan.property) } : plan;
}

/**
 * Serialize one draft as the native property definition an insert action carries.
 *
 * @throws When the draft has no name or a required type reference is missing.
 */
export function notesDataSourcePropertyDefinition(property: NotesDataSourceSchemaPropertyDraft): Record<string, unknown> {
  const definition = notesDataSourceSchemaUpdateFromDraft([property]).properties[property.name.trim()];
  if (typeof definition !== "object" || definition === null || Array.isArray(definition)) {
    throw new Error(`property definition for ${property.name} is missing`);
  }
  return { ...definition };
}

/**
 * Default formula for a new formula property: the first other property that is not computed, or an empty string.
 *
 * @param excludePropertyId The formula property itself.
 * @param drafts Saved drafts in source order.
 */
export function notesDataSourceDefaultFormulaExpression(
  excludePropertyId: string,
  drafts: readonly Pick<NotesDataSourceSchemaPropertyDraft, "id" | "name" | "type">[],
): string {
  const target = drafts.find((property) =>
    property.id !== excludePropertyId && property.type !== "formula" && property.type !== "rollup"
  );
  return target ? `prop("${escapeFormulaPropertyName(target.name)}")` : "\"\"";
}

function escapeFormulaPropertyName(name: string): string {
  return name.replace(/\\/g, "\\\\").replace(/"/g, "\\\"");
}
