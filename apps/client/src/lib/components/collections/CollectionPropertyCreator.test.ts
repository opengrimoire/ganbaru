// @vitest-environment jsdom
import { flushSync, mount, tick, unmount } from "svelte";
import { afterEach, describe, expect, it, vi } from "vitest";
import CollectionPropertyCreator from "./CollectionPropertyCreator.svelte";
import type { CollectionPropertyTypeOption } from "./property-icons";

type Type = "text" | "number" | "relation";
const TYPES: CollectionPropertyTypeOption<Type>[] = [
  { value: "text", label: "Text", kind: "text", section: "basic" },
  { value: "number", label: "Number", kind: "number", section: "basic" },
  { value: "relation", label: "Relation", kind: "relation", section: "advanced" },
];

let component: ReturnType<typeof mount> | undefined;
afterEach(async () => {
  if (component) await unmount(component);
  component = undefined;
  document.body.replaceChildren();
});

/** Mount the creator with a spy for the chosen type. */
function render(props: { pending?: boolean } = {}) {
  const onCreate = vi.fn<(type: Type) => void>();
  component = mount(CollectionPropertyCreator<Type>, { target: document.body, props: { types: TYPES, onCreate, ...props } });
  flushSync();
  return { onCreate };
}

/** Type into an input the way a person would, so bound state updates. */
function type(input: HTMLInputElement, value: string): void {
  input.value = value;
  input.dispatchEvent(new Event("input", { bubbles: true }));
  flushSync();
}

/** Press a key in an input and report whether the creator consumed it. */
function press(input: HTMLInputElement, key: string): KeyboardEvent {
  const event = new KeyboardEvent("keydown", { key, bubbles: true, cancelable: true });
  input.dispatchEvent(event);
  return event;
}

function typeButtons(): HTMLButtonElement[] {
  return [...document.querySelectorAll<HTMLButtonElement>("[data-property-type]")];
}

/** Open the type search and return its field once it has focus. */
async function openSearch(): Promise<HTMLInputElement> {
  document.querySelector<HTMLButtonElement>('button[aria-label="Search types"]')!.click();
  await vi.waitFor(() => expect(document.activeElement).toBe(document.querySelector('input[aria-label="Search types"]')));
  return document.querySelector<HTMLInputElement>('input[aria-label="Search types"]')!;
}

describe("Collection property creator", () => {
  it("creates the clicked type and leaves naming to the draft header", () => {
    const { onCreate } = render();
    expect(document.querySelector('input[aria-label="Property name"]')).toBeNull();
    document.querySelector<HTMLButtonElement>('[data-property-type="number"]')!.click();
    expect(onCreate).toHaveBeenCalledExactlyOnceWith("number");
  });

  it("separates sections with a divider", () => {
    render();
    const grids = document.querySelectorAll(".grid-cols-2");
    expect([...grids].map((grid) => [...grid.querySelectorAll("[data-property-type]")].map((button) => button.getAttribute("data-property-type"))))
      .toEqual([["text", "number"], ["relation"]]);
    expect(document.querySelectorAll(".border-t")).toHaveLength(1);
  });

  it("searches types and creates the first match on Enter", async () => {
    const { onCreate } = render();
    const search = await openSearch();
    type(search, "REL");
    expect(typeButtons().map((button) => button.dataset.propertyType)).toEqual(["relation"]);
    expect(press(search, "a").defaultPrevented).toBe(false);
    expect(press(search, "Enter").defaultPrevented).toBe(true);
    expect(onCreate).toHaveBeenCalledExactlyOnceWith("relation");
  });

  it("says when no type matches and creates nothing on Enter", async () => {
    const { onCreate } = render();
    document.querySelector<HTMLButtonElement>('button[aria-label="Search types"]')!.click();
    await tick();
    const search = document.querySelector<HTMLInputElement>('input[aria-label="Search types"]')!;
    type(search, "missing");
    expect(typeButtons()).toEqual([]);
    expect(document.body.textContent).toContain("No matching types");
    press(search, "Enter");
    expect(onCreate).not.toHaveBeenCalled();
  });

  it("keeps the search toggle from counting as a menu action", () => {
    render();
    expect(document.querySelector('button[aria-label="Search types"]')?.hasAttribute("data-collection-menu-keep-open")).toBe(true);
  });

  it("disables creation while a creation is pending", async () => {
    const { onCreate } = render({ pending: true });
    expect(typeButtons().every((button) => button.disabled)).toBe(true);
    press(await openSearch(), "Enter");
    expect(onCreate).not.toHaveBeenCalled();
  });
});
