import { describe, expect, it } from "vitest";
import type { Translate } from "$lib/i18n/translator.svelte";
import {
  ROUTINE_GROUP_ID,
  effectiveProjectDefaultEventName,
  isBuiltInRoutineProjectId,
  systemProjectGroupName,
  systemProjectName,
} from "./system-defaults";

const spanishLabels: Readonly<Record<string, string>> = {
  "projects.defaults.routine": "Rutina",
  "projects.defaults.learning": "Aprendizaje",
  "projects.defaults.reading": "Lectura",
  "projects.defaults.eating": "Comer",
  "projects.defaults.exercise": "Ejercicio",
  "projects.defaults.hygiene": "Higiene",
  "projects.defaults.commute": "Traslado",
  "projects.defaults.social": "Social",
  "projects.defaults.chores": "Quehaceres",
  "projects.defaults.leisure": "Ocio",
  "projects.defaults.meditate": "Meditar",
  "projects.defaults.health": "Salud",
  "projects.defaults.sleep": "Dormir",
};

const translateSpanish = ((key: string) => spanishLabels[key] ?? key) as Translate;

describe("project system defaults", () => {
  it("localizes the built-in Routine group and projects from stable ids", () => {
    expect(systemProjectGroupName(ROUTINE_GROUP_ID, "Routine", translateSpanish)).toBe("Rutina");
    expect(systemProjectName("project-routine-learning", "Learning", translateSpanish)).toBe("Aprendizaje");
    expect(systemProjectName("project-routine-reading", "Reading", translateSpanish)).toBe("Lectura");
    expect(systemProjectName("project-routine-eat", "Eating", translateSpanish)).toBe("Comer");
    expect(systemProjectName("project-routine-health", "Health", translateSpanish)).toBe("Salud");
    expect(systemProjectName("project-routine-commute", "Commute", translateSpanish)).toBe("Traslado");
    expect(systemProjectName("project-routine-sleep", "Sleep", translateSpanish)).toBe("Dormir");
  });

  it("leaves user-created names unchanged", () => {
    expect(systemProjectGroupName("group-custom", "Rutina personal", translateSpanish)).toBe("Rutina personal");
    expect(systemProjectName("project-custom", "Desayuno", translateSpanish)).toBe("Desayuno");
    expect(isBuiltInRoutineProjectId("project-custom")).toBe(false);
  });

  it("uses the translated system name until the default event name is customized", () => {
    expect(effectiveProjectDefaultEventName({
      id: "project-routine-eat",
      name: "Comer",
      defaultEventName: null,
    })).toBe("Comer");
    expect(effectiveProjectDefaultEventName({
      id: "project-routine-eat",
      name: "Comer",
      defaultEventName: "Almuerzo",
    })).toBe("Almuerzo");
    expect(effectiveProjectDefaultEventName({
      id: "project-custom",
      name: "Desayuno",
      defaultEventName: null,
    })).toBeNull();
  });
});
