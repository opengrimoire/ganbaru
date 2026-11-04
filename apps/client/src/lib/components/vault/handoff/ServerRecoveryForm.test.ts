// @vitest-environment jsdom

import { flushSync, mount, tick, unmount } from "svelte";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { VaultBootstrapOutcome } from "$lib/api/vault-handoff";
import ServerRecoveryForm from "./ServerRecoveryForm.svelte";

const recoverVaultThroughServer = vi.hoisted(() => vi.fn<(code: string, kit: string) => Promise<VaultBootstrapOutcome>>());
vi.mock("$lib/api/vault-handoff", () => ({ recoverVaultThroughServer }));

const CODE = "ganbaru-anchor:abc";
const KIT = "AAAA-BBBB-CCCC";

describe("ServerRecoveryForm", () => {
  let target: HTMLDivElement;
  let component: ReturnType<typeof mount> | null = null;
  const onActivated = vi.fn();

  beforeEach(() => {
    recoverVaultThroughServer.mockReset();
    onActivated.mockReset();
    target = document.createElement("div");
    document.body.append(target);
    component = mount(ServerRecoveryForm, { target, props: { buttonClass: "", onActivated } });
    flushSync();
  });

  afterEach(async () => {
    if (component) await unmount(component);
    target.remove();
    component = null;
    vi.restoreAllMocks();
  });

  function field(id: string): HTMLInputElement {
    const input = target.querySelector<HTMLInputElement>(`#${id}`);
    if (!input) throw new Error(`missing field ${id}`);
    return input;
  }

  function enter(id: string, value: string): void {
    const input = field(id);
    input.value = value;
    input.dispatchEvent(new Event("input", { bubbles: true }));
    flushSync();
  }

  function submitButton(): HTMLButtonElement {
    const button = target.querySelector<HTMLButtonElement>('button[type="submit"]');
    if (!button) throw new Error("missing submit button");
    return button;
  }

  async function submit(): Promise<void> {
    target.querySelector("form")?.dispatchEvent(new Event("submit", { bubbles: true, cancelable: true }));
    await tick();
    await vi.waitFor(() => expect(field("server-recovery-code").disabled).toBe(false));
  }

  function openForm(): void {
    target.querySelector<HTMLButtonElement>("button")?.click();
    flushSync();
  }

  it("stays closed until asked for and needs both the code and the kit", () => {
    expect(target.querySelector("form")).toBeNull();
    openForm();
    expect(submitButton().disabled).toBe(true);
    enter("server-recovery-code", CODE);
    expect(submitButton().disabled).toBe(true);
    enter("server-recovery-kit", KIT);
    expect(submitButton().disabled).toBe(false);
  });

  it("recovers with the trimmed code and reports activation with both fields cleared", async () => {
    recoverVaultThroughServer.mockResolvedValue({ transferId: "t", activated: true, inProgress: false });
    openForm();
    enter("server-recovery-code", `  ${CODE}\n`);
    enter("server-recovery-kit", KIT);
    await submit();
    expect(recoverVaultThroughServer).toHaveBeenCalledWith(CODE, KIT);
    expect(onActivated).toHaveBeenCalledOnce();
    expect(field("server-recovery-code").value).toBe("");
    expect(field("server-recovery-kit").value).toBe("");
  });

  it("forgets the kit after a refusal, keeps the code, and logs nothing", async () => {
    const warn = vi.spyOn(console, "warn");
    const failure = vi.spyOn(console, "error");
    recoverVaultThroughServer.mockRejectedValue("read the recovery kit: invalid length");
    openForm();
    enter("server-recovery-code", CODE);
    enter("server-recovery-kit", KIT);
    await submit();
    expect(onActivated).not.toHaveBeenCalled();
    expect(field("server-recovery-kit").value).toBe("");
    expect(field("server-recovery-code").value).toBe(CODE);
    expect(target.textContent).not.toContain(KIT);
    expect(target.textContent).toContain("recovery kit");
    expect(warn).not.toHaveBeenCalled();
    expect(failure).not.toHaveBeenCalled();
  });

  it("does not report activation while another copy is running", async () => {
    recoverVaultThroughServer.mockResolvedValue({ transferId: null, activated: false, inProgress: true });
    openForm();
    enter("server-recovery-code", CODE);
    enter("server-recovery-kit", KIT);
    await submit();
    expect(onActivated).not.toHaveBeenCalled();
    expect(target.querySelector('[role="status"]')).not.toBeNull();
  });
});
