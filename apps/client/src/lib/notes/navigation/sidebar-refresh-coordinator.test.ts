import { describe, expect, it, vi } from "vitest";
import { createNotesSidebarRefreshCoordinator } from "./sidebar-refresh-coordinator";

describe("Notes sidebar refresh coordinator", () => {
  it("coalesces repeated metadata and hierarchy changes into one bounded read", async () => {
    vi.useFakeTimers();
    const refresh = vi.fn(async () => undefined);
    const coordinator = createNotesSidebarRefreshCoordinator({ refresh });
    coordinator.schedule("visible-metadata");
    coordinator.schedule("visible-metadata");
    coordinator.schedule("hierarchy");

    expect(refresh).not.toHaveBeenCalled();
    await vi.runAllTimersAsync();

    expect(refresh).toHaveBeenCalledTimes(1);
    expect(refresh).toHaveBeenCalledWith("hierarchy");
    vi.useRealTimers();
  });

  it("does not schedule a read for content-only mutations", async () => {
    vi.useFakeTimers();
    const refresh = vi.fn(async () => undefined);
    const coordinator = createNotesSidebarRefreshCoordinator({ refresh });
    coordinator.schedule("none");
    await vi.runAllTimersAsync();
    expect(refresh).not.toHaveBeenCalled();
    vi.useRealTimers();
  });
});
